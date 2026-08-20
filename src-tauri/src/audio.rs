use serde::Serialize;
use windows::{
    core::*,
    Win32::Media::Audio::*,
    Win32::Media::Audio::Endpoints::{IAudioEndpointVolume, IAudioMeterInformation},
    Win32::System::Com::*,
};

use windows::Win32::Devices::FunctionDiscovery::PKEY_Device_FriendlyName;
use windows::Win32::System::Com::StructuredStorage::PropVariantToStringAlloc;
use windows::Win32::UI::Shell::PropertiesSystem::IPropertyStore;

#[derive(Serialize)]
pub struct AudioDevice {
    pub id: String,
    pub name: String,
    pub detail: String,
    pub available: bool,
}

pub fn initialize_com() -> Result<()> {
    unsafe {
        // s_ok or s_false (already initialized) are both fine
        let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
    }
    Ok(())
}

fn get_device_enumerator() -> Result<IMMDeviceEnumerator> {
    unsafe { CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL) }
}

fn get_device_by_id(id: &str) -> Result<IMMDevice> {
    let enumerator = get_device_enumerator()?;
    let hstr = HSTRING::from(id);
    unsafe { enumerator.GetDevice(PCWSTR::from_raw(hstr.as_ptr())) }
}

fn get_default_mic() -> Result<IMMDevice> {
    let enumerator = get_device_enumerator()?;
    unsafe { enumerator.GetDefaultAudioEndpoint(eCapture, eMultimedia) }
}

// falls back to a numbered label if the property store lookup fails
fn friendly_name(device: &IMMDevice, index: u32) -> String {
    unsafe {
        let store: Result<IPropertyStore> = device.OpenPropertyStore(STGM_READ);
        let name = store.ok().and_then(|store| {
            let prop = store.GetValue(&PKEY_Device_FriendlyName).ok()?;
            let pwstr = PropVariantToStringAlloc(&prop).ok()?;
            let s = pwstr.to_string().ok();
            CoTaskMemFree(Some(pwstr.0 as *const _));
            s
        });
        name.unwrap_or_else(|| format!("Microphone {}", index + 1))
    }
}

pub fn get_devices() -> Result<Vec<AudioDevice>> {
    let enumerator = get_device_enumerator()?;
    let collection = unsafe { enumerator.EnumAudioEndpoints(eCapture, DEVICE_STATE_ACTIVE) }?;

    let count = unsafe { collection.GetCount() }?;
    let mut devices = Vec::new();

    let default_id = get_default_mic()
        .ok()
        .and_then(|d| unsafe { d.GetId() }.ok())
        .and_then(|p| unsafe { p.to_string() }.ok());

    for i in 0..count {
        if let Ok(device) = unsafe { collection.Item(i) } {
            let id_pwstr = unsafe { device.GetId() }.unwrap_or_default();
            let id = unsafe { id_pwstr.to_string() }.unwrap_or_default();
            let name = friendly_name(&device, i);

            let detail = if default_id.as_deref() == Some(id.as_str()) {
                "Default microphone".to_string()
            } else {
                "Available".to_string()
            };

            devices.push(AudioDevice {
                id,
                name,
                detail,
                available: true,
            });
        }
    }

    Ok(devices)
}

// shared so other modules (sessions, peak metering) can activate their own interface
// off the device without each repeating this same default mic fallback
pub fn get_device(device_id: &str) -> Result<IMMDevice> {
    if device_id.is_empty() {
        get_default_mic()
    } else {
        get_device_by_id(device_id).or_else(|_| get_default_mic())
    }
}

fn endpoint_volume(device_id: &str) -> Result<IAudioEndpointVolume> {
    let device = get_device(device_id)?;
    unsafe { device.Activate(CLSCTX_ALL, None) }
}

pub fn set_mute(device_id: &str, muted: bool) -> Result<()> {
    let vol = endpoint_volume(device_id)?;
    unsafe { vol.SetMute(muted, std::ptr::null()) }
}

pub fn set_volume(device_id: &str, level: f32) -> Result<()> {
    let vol = endpoint_volume(device_id)?;
    unsafe { vol.SetMasterVolumeLevelScalar(level, std::ptr::null()) }
}

pub fn get_mute(device_id: &str) -> Result<bool> {
    let vol = endpoint_volume(device_id)?;
    let muted = unsafe { vol.GetMute()? };
    Ok(muted.as_bool())
}

// windows gives this back as 0.0-1.0, not a percent
pub fn get_volume_level(device_id: &str) -> Result<f32> {
    let vol = endpoint_volume(device_id)?;
    let level = unsafe { vol.GetMasterVolumeLevelScalar()? };
    Ok(level)
}

// windows only updates the peak meter while another app is also capturing this device
// a real capture based meter would be more accurate but is a bigger lift for later
pub fn get_peak_level(device_id: &str) -> Result<f32> {
    let device = get_device(device_id)?;
    let meter: IAudioMeterInformation = unsafe { device.Activate(CLSCTX_ALL, None) }?;
    let peak = unsafe { meter.GetPeakValue() }?;
    Ok(peak)
}