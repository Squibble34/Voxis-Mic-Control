use std::io::{self, Read, Write};

use crate::descriptors::MicDescriptors;

pub const USBIP_VERSION: u16 = 0x0111;
pub const OP_REQ_DEVLIST: u16 = 0x8005;
pub const OP_REP_DEVLIST: u16 = 0x0005;
pub const OP_REQ_IMPORT: u16 = 0x8003;
pub const SPEED_FULL: u32 = 2;
pub const OP_REP_IMPORT: u16 = 0x0003;
pub const CMD_SUBMIT: u32 = 1;
pub const CMD_UNLINK: u32 = 2;
pub const RET_SUBMIT: u32 = 3;
pub const RET_UNLINK: u32 = 4;
pub const DIRECTION_OUT: u32 = 0;
pub const DIRECTION_IN: u32 = 1;
// the wire uses all ones for urbs that are not isochronous, it is not an error
pub const NO_ISO_PACKETS: u32 = 0xFFFF_FFFF;
pub const STATUS_OK: i32 = 0;
pub const STATUS_PIPE: i32 = -32;
const MAX_TRANSFER_BYTES: u32 = 16 * 1024 * 1024;
const MAX_ISO_PACKETS: u32 = 4096;

pub struct OpHeader {
    pub version: u16,
    pub code: u16,
    pub _status: u32,
}

pub fn read_op_header(reader: &mut impl Read) -> io::Result<OpHeader> {
    let mut raw = [0u8; 8];
    reader.read_exact(&mut raw)?;
    let header = OpHeader {
        version: u16::from_be_bytes([raw[0], raw[1]]),
        code: u16::from_be_bytes([raw[2], raw[3]]),
        _status: u32::from_be_bytes([raw[4], raw[5], raw[6], raw[7]]),
    };
    if header.version != USBIP_VERSION {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("unsupported usbip version 0x{:04x}", header.version),
        ));
    }
    Ok(header)
}

pub fn write_op_header(writer: &mut impl Write, code: u16, status: u32) -> io::Result<()> {
    writer.write_all(&USBIP_VERSION.to_be_bytes())?;
    writer.write_all(&code.to_be_bytes())?;
    writer.write_all(&status.to_be_bytes())
}

#[derive(Clone, Copy)]
pub struct UrbHeader {
    pub command: u32,
    pub sequence: u32,
    pub direction: u32,
    pub endpoint: u32,
}

#[derive(Clone)]
pub struct IsoPacket {
    pub offset: u32,
    pub length: u32,
    pub actual_length: u32,
    pub status: i32,
}

pub struct SubmitRequest {
    pub header: UrbHeader,
    pub buffer_length: u32,
    pub start_frame: u32,
    pub packet_count: u32,
    pub setup: [u8; 8],
    pub out_payload: Vec<u8>,
    pub iso_packets: Vec<IsoPacket>,
}

impl SubmitRequest {
    pub fn is_isochronous(&self) -> bool {
        self.packet_count != NO_ISO_PACKETS && self.packet_count != 0
    }
}

fn be_u32(raw: &[u8], at: usize) -> u32 {
    u32::from_be_bytes([raw[at], raw[at + 1], raw[at + 2], raw[at + 3]])
}

fn invalid_data(message: String) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message)
}

pub fn read_urb_header(reader: &mut impl Read) -> io::Result<UrbHeader> {
    let mut raw = [0u8; 20];
    reader.read_exact(&mut raw)?;
    // bytes 8 to 12 are the device id, which we dont need
    Ok(UrbHeader {
        command: be_u32(&raw, 0),
        sequence: be_u32(&raw, 4),
        direction: be_u32(&raw, 12),
        endpoint: be_u32(&raw, 16),
    })
}

pub fn read_submit_body_into(
    reader: &mut impl Read,
    header: UrbHeader,
    request: &mut SubmitRequest,
) -> io::Result<()> {
    let mut raw = [0u8; 28];
    reader.read_exact(&mut raw)?;
    let buffer_length = be_u32(&raw, 4);
    let start_frame = be_u32(&raw, 8);
    let packet_count = be_u32(&raw, 12);
    let mut setup = [0u8; 8];
    setup.copy_from_slice(&raw[20..28]);

    if buffer_length > MAX_TRANSFER_BYTES {
        return Err(invalid_data(format!("transfer length {buffer_length} too large")));
    }
    request.header = header;
    request.buffer_length = buffer_length;
    request.start_frame = start_frame;
    request.packet_count = packet_count;
    request.setup = setup;
    request.out_payload.clear();
    if header.direction == DIRECTION_OUT && buffer_length > 0 {
        request.out_payload.resize(buffer_length as usize, 0);
        reader.read_exact(&mut request.out_payload)?;
    }
    read_iso_packets_into(reader, packet_count, &mut request.iso_packets)?;
    Ok(())
}

fn read_iso_packets_into(
    reader: &mut impl Read,
    packet_count: u32,
    packets: &mut Vec<IsoPacket>,
) -> io::Result<()> {
    packets.clear();
    if packet_count == NO_ISO_PACKETS || packet_count == 0 {
        return Ok(());
    }
    if packet_count > MAX_ISO_PACKETS {
        return Err(invalid_data(format!("invalid isochronous packet count {packet_count}")));
    }
    packets.reserve(packet_count as usize);
    for _ in 0..packet_count {
        let mut raw = [0u8; 16];
        reader.read_exact(&mut raw)?;
        packets.push(IsoPacket {
            offset: be_u32(&raw, 0),
            length: be_u32(&raw, 4),
            actual_length: be_u32(&raw, 8),
            status: be_u32(&raw, 12) as i32,
        });
    }
    Ok(())
}

pub fn read_unlink_body(reader: &mut impl Read) -> io::Result<u32> {
    let mut raw = [0u8; 28];
    reader.read_exact(&mut raw)?;
    Ok(be_u32(&raw, 0))
}

pub fn write_ret_submit_reusable(
    writer: &mut impl Write,
    request: &SubmitRequest,
    status: i32,
    actual_length: u32,
    payload: &[u8],
    iso_packets: &[IsoPacket],
    frame: &mut Vec<u8>,
) -> io::Result<()> {
    let failed = status != STATUS_OK;
    let actual_length = if failed { 0 } else { actual_length };
    let payload: &[u8] = if failed { &[] } else { payload };

    let packet_count = if request.is_isochronous() {
        iso_packets.len() as u32
    } else {
        NO_ISO_PACKETS
    };
    let start_frame = if request.is_isochronous() { request.start_frame } else { 0 };
    let error_count = iso_packets.iter().filter(|packet| packet.status != STATUS_OK).count() as u32;

    frame.clear();
    frame.reserve(48 + payload.len() + iso_packets.len() * 16);
    frame.extend_from_slice(&RET_SUBMIT.to_be_bytes());
    frame.extend_from_slice(&request.header.sequence.to_be_bytes());
    // device id, direction and endpoint must be zero in replies
    frame.extend_from_slice(&[0u8; 12]);
    frame.extend_from_slice(&status.to_be_bytes());
    frame.extend_from_slice(&actual_length.to_be_bytes());
    frame.extend_from_slice(&start_frame.to_be_bytes());
    frame.extend_from_slice(&packet_count.to_be_bytes());
    frame.extend_from_slice(&error_count.to_be_bytes());
    frame.extend_from_slice(&[0u8; 8]);
    frame.extend_from_slice(payload);
    for packet in iso_packets {
        frame.extend_from_slice(&packet.offset.to_be_bytes());
        frame.extend_from_slice(&packet.length.to_be_bytes());
        frame.extend_from_slice(&packet.actual_length.to_be_bytes());
        frame.extend_from_slice(&packet.status.to_be_bytes());
    }
    writer.write_all(frame)
}

pub fn write_ret_unlink(writer: &mut impl Write, sequence: u32, status: i32) -> io::Result<()> {
    let mut frame: Vec<u8> = Vec::with_capacity(48);
    frame.extend_from_slice(&RET_UNLINK.to_be_bytes());
    frame.extend_from_slice(&sequence.to_be_bytes());
    frame.extend_from_slice(&[0u8; 12]);
    frame.extend_from_slice(&status.to_be_bytes());
    frame.extend_from_slice(&[0u8; 24]);
    writer.write_all(&frame)
}

pub fn write_usb_device(
    writer: &mut impl Write,
    bus_id: &str,
    device_number: u32,
    descriptors: &MicDescriptors,
) -> io::Result<()> {
    let mut sysfs_path = [0u8; 256];
    let mut bus_id_field = [0u8; 32];
    copy_padded(&mut sysfs_path, &format!("/sys/devices/platform/voxis/{bus_id}"));
    copy_padded(&mut bus_id_field, bus_id);

    let mut block: Vec<u8> = Vec::with_capacity(312);
    block.extend_from_slice(&sysfs_path);
    block.extend_from_slice(&bus_id_field);
    block.extend_from_slice(&1u32.to_be_bytes());
    block.extend_from_slice(&device_number.to_be_bytes());
    block.extend_from_slice(&SPEED_FULL.to_be_bytes());
    block.extend_from_slice(&descriptors.vendor_id().to_be_bytes());
    block.extend_from_slice(&descriptors.product_id().to_be_bytes());
    block.extend_from_slice(&descriptors.device_release().to_be_bytes());
    block.extend_from_slice(&descriptors.device_class_triplet());
    block.push(1);
    block.push(descriptors.configuration_count());
    block.push(descriptors.interface_count());
    writer.write_all(&block)
}

fn copy_padded(field: &mut [u8], text: &str) {
    let raw = text.as_bytes();
    let length = raw.len().min(field.len() - 1);
    field[..length].copy_from_slice(&raw[..length]);
}
