use serde::Serialize;
use windows::{
    core::*,
    Win32::Media::Audio::*,
    Win32::Media::Audio::Endpoints::IAudioEndpointVolume,
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

pub fn is_voxis_virtual_mic(name: &str) -> bool {
    name.contains("Voxis Microphone")
}

pub fn find_voxis_virtual_mic_id() -> Option<String> {
    let enumerator = get_device_enumerator().ok()?;
    let collection = unsafe { enumerator.EnumAudioEndpoints(eCapture, DEVICE_STATE_ACTIVE) }.ok()?;
    let count = unsafe { collection.GetCount() }.ok()?;

    for i in 0..count {
        if let Ok(device) = unsafe { collection.Item(i) } {
            let name = friendly_name(&device, i);
            if is_voxis_virtual_mic(&name) {
                let id_pwstr = unsafe { device.GetId() }.ok()?;
                return unsafe { id_pwstr.to_string() }.ok();
            }
        }
    }
    None
}

// ipolicyconfig is an undocumented com interface, slot 13 in its vtable is setdefaultendpoint
pub fn set_voxis_as_default_mic() -> Result<()> {
    let _ = initialize_com();
    let Some(voxis_id) = find_voxis_virtual_mic_id() else {
        return Err(Error::from_hresult(HRESULT(-1)));
    };

    const CLSID_POLICY_CONFIG_CLIENT: GUID = GUID::from_u128(0x870af99c_171d_4f9e_af0d_e63df40c2bc9);
    const IID_IPOLICY_CONFIG: GUID = GUID::from_u128(0xf8679f50_850a_41cf_9c72_430f290290c8);

    unsafe {
        let unknown: IUnknown = CoCreateInstance(&CLSID_POLICY_CONFIG_CLIENT, None, CLSCTX_ALL)?;
        let mut policy_ptr: *mut core::ffi::c_void = std::ptr::null_mut();
        unknown.query(&IID_IPOLICY_CONFIG, &mut policy_ptr).ok()?;

        let vtable = *(policy_ptr as *const *const *const core::ffi::c_void);
        let set_default_endpoint: unsafe extern "system" fn(
            *mut core::ffi::c_void,
            PCWSTR,
            ERole,
        ) -> HRESULT = std::mem::transmute(*vtable.add(13));
        let release: unsafe extern "system" fn(*mut core::ffi::c_void) -> u32 =
            std::mem::transmute(*vtable.add(2));

        let hstr = HSTRING::from(voxis_id.as_str());
        let raw_id = PCWSTR::from_raw(hstr.as_ptr());
        let hr_console = set_default_endpoint(policy_ptr, raw_id, eConsole);
        let _ = set_default_endpoint(policy_ptr, raw_id, eMultimedia);
        let _ = set_default_endpoint(policy_ptr, raw_id, eCommunications);
        release(policy_ptr);

        hr_console.ok()
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
            if is_voxis_virtual_mic(&name) {
                continue;
            }

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

pub fn get_mute(device_id: &str) -> Result<bool> {
    let vol = endpoint_volume(device_id)?;
    let muted = unsafe { vol.GetMute()? };
    Ok(muted.as_bool())
}