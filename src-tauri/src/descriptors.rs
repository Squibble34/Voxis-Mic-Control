pub const DESC_DEVICE: u8 = 0x01;
pub const DESC_CONFIGURATION: u8 = 0x02;
pub const DESC_STRING: u8 = 0x03;
pub const DESC_INTERFACE: u8 = 0x04;
pub const DESC_ENDPOINT: u8 = 0x05;
pub const DESC_CS_INTERFACE: u8 = 0x24;
pub const DESC_CS_ENDPOINT: u8 = 0x25;

pub const INPUT_TERMINAL_ID: u8 = 1;
pub const FEATURE_UNIT_ID: u8 = 2;
pub const OUTPUT_TERMINAL_ID: u8 = 3;
pub const CAPTURE_ENDPOINT: u8 = 0x81;

pub struct MicDescriptors {
    pub device: Vec<u8>,
    pub configuration: Vec<u8>,
    pub strings: Vec<Vec<u8>>,
}

impl MicDescriptors {
    pub fn new() -> Self {
        MicDescriptors {
            device: device_descriptor(),
            configuration: configuration_descriptor(),
            strings: vec![
                vec![4, DESC_STRING, 0x09, 0x04],
                string_descriptor("Voxis"),
                string_descriptor("Voxis Microphone"),
                string_descriptor("VOXIS-MIC-001"),
            ],
        }
    }

    pub fn vendor_id(&self) -> u16 {
        u16::from_le_bytes([self.device[8], self.device[9]])
    }

    pub fn product_id(&self) -> u16 {
        u16::from_le_bytes([self.device[10], self.device[11]])
    }

    pub fn device_release(&self) -> u16 {
        u16::from_le_bytes([self.device[12], self.device[13]])
    }

    pub fn device_class_triplet(&self) -> [u8; 3] {
        [self.device[4], self.device[5], self.device[6]]
    }

    pub fn configuration_count(&self) -> u8 {
        self.device[17]
    }

    pub fn interface_count(&self) -> u8 {
        self.configuration[4]
    }

    pub fn interface_summaries(&self) -> Vec<[u8; 3]> {
        let mut summaries = Vec::new();
        let mut pos = 0;
        while pos + 7 < self.configuration.len() {
            let length = self.configuration[pos] as usize;
            if length < 2 {
                break;
            }
            let is_first_setting = self.configuration[pos + 3] == 0;
            if self.configuration[pos + 1] == DESC_INTERFACE && is_first_setting {
                summaries.push([
                    self.configuration[pos + 5],
                    self.configuration[pos + 6],
                    self.configuration[pos + 7],
                ]);
            }
            pos += length;
        }
        summaries
    }
}

fn device_descriptor() -> Vec<u8> {
    let mut descriptor = vec![0u8; 18];
    descriptor[0] = 18;
    descriptor[1] = DESC_DEVICE;
    descriptor[2..4].copy_from_slice(&0x0110u16.to_le_bytes());
    // class zero means each interface declares its own class
    descriptor[7] = 64;
    descriptor[8..10].copy_from_slice(&0xFFFFu16.to_le_bytes());
    descriptor[10..12].copy_from_slice(&0xB001u16.to_le_bytes());
    descriptor[12..14].copy_from_slice(&0x0001u16.to_le_bytes());
    descriptor[14] = 1;
    descriptor[15] = 2;
    descriptor[16] = 3;
    descriptor[17] = 1;
    descriptor
}

fn configuration_descriptor() -> Vec<u8> {
    let mut descriptor: Vec<u8> = Vec::new();

    descriptor.extend_from_slice(&[9, DESC_CONFIGURATION, 0, 0, 2, 1, 0, 0x80, 50]);

    descriptor.extend_from_slice(&[9, DESC_INTERFACE, 0, 0, 0, 0x01, 0x01, 0x00, 0]);

    let control_start = descriptor.len();
    descriptor.extend_from_slice(&[9, DESC_CS_INTERFACE, 0x01, 0x00, 0x01, 0, 0, 1, 1]);
    descriptor.extend_from_slice(&[
        12, DESC_CS_INTERFACE, 0x02, INPUT_TERMINAL_ID, 0x01, 0x02, 0, 1, 0x01, 0x00, 0, 0,
    ]);
    descriptor.extend_from_slice(&[
        9, DESC_CS_INTERFACE, 0x06, FEATURE_UNIT_ID, INPUT_TERMINAL_ID, 1, 0x03, 0x00, 0,
    ]);
    descriptor.extend_from_slice(&[
        9, DESC_CS_INTERFACE, 0x03, OUTPUT_TERMINAL_ID, 0x01, 0x01, 0, FEATURE_UNIT_ID, 0,
    ]);
    let control_length = (descriptor.len() - control_start) as u16;
    descriptor[control_start + 5..control_start + 7].copy_from_slice(&control_length.to_le_bytes());

    // setting 0 has no endpoint so the mic costs no bandwidth until an app opens it
    descriptor.extend_from_slice(&[9, DESC_INTERFACE, 1, 0, 0, 0x01, 0x02, 0x00, 0]);
    descriptor.extend_from_slice(&[9, DESC_INTERFACE, 1, 1, 1, 0x01, 0x02, 0x00, 0]);
    descriptor.extend_from_slice(&[7, DESC_CS_INTERFACE, 0x01, OUTPUT_TERMINAL_ID, 1, 0x01, 0x00]);
    descriptor.extend_from_slice(&[11, DESC_CS_INTERFACE, 0x02, 1, 1, 2, 16, 1, 0x80, 0xBB, 0x00]);
    descriptor.extend_from_slice(&[9, DESC_ENDPOINT, CAPTURE_ENDPOINT, 0x0D, 0x60, 0x00, 1, 0, 0]);
    descriptor.extend_from_slice(&[7, DESC_CS_ENDPOINT, 0x01, 0, 0, 0, 0]);

    let total_length = descriptor.len() as u16;
    descriptor[2..4].copy_from_slice(&total_length.to_le_bytes());
    descriptor
}

fn string_descriptor(text: &str) -> Vec<u8> {
    let units: Vec<u16> = text.encode_utf16().collect();
    let mut descriptor = vec![(2 + units.len() * 2) as u8, DESC_STRING];
    for unit in units {
        descriptor.extend_from_slice(&unit.to_le_bytes());
    }
    descriptor
}
