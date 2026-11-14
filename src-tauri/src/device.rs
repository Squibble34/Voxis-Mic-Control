use crate::descriptors::{
    MicDescriptors, CAPTURE_ENDPOINT, DESC_CONFIGURATION, DESC_DEVICE, DESC_STRING,
    FEATURE_UNIT_ID,
};

const REQ_GET_STATUS: u8 = 0x00;
const REQ_CLEAR_FEATURE: u8 = 0x01;
const REQ_SET_FEATURE: u8 = 0x03;
const REQ_SET_ADDRESS: u8 = 0x05;
const REQ_GET_DESCRIPTOR: u8 = 0x06;
const REQ_GET_CONFIGURATION: u8 = 0x08;
const REQ_SET_CONFIGURATION: u8 = 0x09;
const REQ_GET_INTERFACE: u8 = 0x0A;
const REQ_SET_INTERFACE: u8 = 0x0B;
const REQ_SYNCH_FRAME: u8 = 0x0C;

const AUDIO_SET_CUR: u8 = 0x01;
const AUDIO_GET_CUR: u8 = 0x81;
const AUDIO_GET_MIN: u8 = 0x82;
const AUDIO_GET_MAX: u8 = 0x83;
const AUDIO_GET_RES: u8 = 0x84;

const SAMPLE_RATE_HZ: u32 = 48_000;
const STREAMING_INTERFACE: u8 = 1;

pub struct SetupPacket {
    pub request_type: u8,
    pub request: u8,
    pub value: u16,
    pub index: u16,
    pub length: u16,
}

impl SetupPacket {
    pub fn parse(raw: &[u8; 8]) -> Self {
        SetupPacket {
            request_type: raw[0],
            request: raw[1],
            value: u16::from_le_bytes([raw[2], raw[3]]),
            index: u16::from_le_bytes([raw[4], raw[5]]),
            length: u16::from_le_bytes([raw[6], raw[7]]),
        }
    }
}

pub struct MicDevice<'a> {
    descriptors: &'a MicDescriptors,
    configuration: u8,
    streaming_setting: u8,
    muted: bool,
    volume_256ths_db: i16,
}

impl<'a> MicDevice<'a> {
    pub fn new(descriptors: &'a MicDescriptors) -> Self {
        MicDevice {
            descriptors,
            configuration: 0,
            streaming_setting: 0,
            muted: false,
            volume_256ths_db: 0,
        }
    }

    // none means stall, some holds the bytes to send back (empty when there is nothing to return)
    pub fn handle_control(&mut self, setup: &SetupPacket, out_payload: &[u8]) -> Option<Vec<u8>> {
        let is_standard = (setup.request_type & 0x60) == 0x00;
        if is_standard {
            self.handle_standard(setup)
        } else {
            self.handle_audio_class(setup, out_payload)
        }
    }

    fn handle_standard(&mut self, setup: &SetupPacket) -> Option<Vec<u8>> {
        match setup.request {
            REQ_GET_DESCRIPTOR => {
                let descriptor_type = (setup.value >> 8) as u8;
                let descriptor_index = setup.value as u8;
                let descriptor = match (descriptor_type, descriptor_index) {
                    (DESC_DEVICE, 0) => self.descriptors.device.clone(),
                    (DESC_CONFIGURATION, 0) => self.descriptors.configuration.clone(),
                    (DESC_STRING, index) => self.descriptors.strings.get(index as usize)?.clone(),
                    _ => return None,
                };
                Some(clipped(descriptor, setup.length))
            }
            REQ_SET_ADDRESS | REQ_CLEAR_FEATURE | REQ_SET_FEATURE => Some(Vec::new()),
            REQ_SET_CONFIGURATION => {
                let wanted = setup.value as u8;
                if wanted > 1 {
                    return None;
                }
                self.configuration = wanted;
                self.streaming_setting = 0;
                Some(Vec::new())
            }
            REQ_GET_CONFIGURATION => Some(clipped(vec![self.configuration], setup.length)),
            REQ_SET_INTERFACE => {
                let interface = setup.index as u8;
                let setting = setup.value as u8;
                let valid = self.configuration == 1
                    && ((interface == 0 && setting == 0)
                        || (interface == STREAMING_INTERFACE && setting <= 1));
                if !valid {
                    return None;
                }
                if interface == STREAMING_INTERFACE {
                    self.streaming_setting = setting;
                }
                Some(Vec::new())
            }
            REQ_GET_INTERFACE => {
                let setting = match setup.index as u8 {
                    0 => 0,
                    STREAMING_INTERFACE => self.streaming_setting,
                    _ => return None,
                };
                Some(clipped(vec![setting], setup.length))
            }
            REQ_GET_STATUS | REQ_SYNCH_FRAME => Some(clipped(vec![0, 0], setup.length)),
            _ => None,
        }
    }

    fn handle_audio_class(&mut self, setup: &SetupPacket, out_payload: &[u8]) -> Option<Vec<u8>> {
        let recipient = setup.request_type & 0x1F;
        let selector = (setup.value >> 8) as u8;

        if recipient == 0x02 && selector == 0x01 && (setup.index as u8) == CAPTURE_ENDPOINT {
            return handle_sample_rate(setup, out_payload);
        }

        let interface = setup.index as u8;
        let entity_id = (setup.index >> 8) as u8;
        if recipient == 0x01 && interface == 0 && entity_id == FEATURE_UNIT_ID {
            return self.handle_feature_unit(selector, setup, out_payload);
        }
        None
    }

    fn handle_feature_unit(
        &mut self,
        selector: u8,
        setup: &SetupPacket,
        out_payload: &[u8],
    ) -> Option<Vec<u8>> {
        const MUTE_CONTROL: u8 = 0x01;
        const VOLUME_CONTROL: u8 = 0x02;

        match (selector, setup.request) {
            (MUTE_CONTROL, AUDIO_SET_CUR) => {
                if let Some(&flag) = out_payload.first() {
                    self.muted = flag != 0;
                }
                Some(Vec::new())
            }
            (MUTE_CONTROL, AUDIO_GET_CUR) => Some(clipped(vec![self.muted as u8], setup.length)),
            (VOLUME_CONTROL, AUDIO_SET_CUR) => {
                if out_payload.len() >= 2 {
                    self.volume_256ths_db = i16::from_le_bytes([out_payload[0], out_payload[1]]);
                }
                Some(Vec::new())
            }
            (VOLUME_CONTROL, AUDIO_GET_CUR) => {
                Some(clipped(self.volume_256ths_db.to_le_bytes().to_vec(), setup.length))
            }
            (VOLUME_CONTROL, AUDIO_GET_MIN) => {
                Some(clipped((-60i16 * 256).to_le_bytes().to_vec(), setup.length))
            }
            (VOLUME_CONTROL, AUDIO_GET_MAX) => {
                Some(clipped(0i16.to_le_bytes().to_vec(), setup.length))
            }
            (VOLUME_CONTROL, AUDIO_GET_RES) => {
                Some(clipped(256i16.to_le_bytes().to_vec(), setup.length))
            }
            _ => None,
        }
    }
}

fn handle_sample_rate(setup: &SetupPacket, out_payload: &[u8]) -> Option<Vec<u8>> {
    match setup.request {
        AUDIO_SET_CUR => {
            if out_payload.len() >= 3 {
                let rate = u32::from_le_bytes([out_payload[0], out_payload[1], out_payload[2], 0]);
                if rate != SAMPLE_RATE_HZ {
                    return None;
                }
            }
            Some(Vec::new())
        }
        AUDIO_GET_CUR | AUDIO_GET_MIN | AUDIO_GET_MAX => {
            Some(clipped(rate_bytes(SAMPLE_RATE_HZ), setup.length))
        }
        AUDIO_GET_RES => Some(clipped(rate_bytes(1), setup.length)),
        _ => None,
    }
}

fn rate_bytes(rate: u32) -> Vec<u8> {
    vec![rate as u8, (rate >> 8) as u8, (rate >> 16) as u8]
}

fn clipped(mut payload: Vec<u8>, max_length: u16) -> Vec<u8> {
    payload.truncate(max_length as usize);
    payload
}
