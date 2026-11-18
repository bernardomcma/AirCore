//! Windows Core Audio microphone endpoint control.
//!
//! Allows disabling/enabling the Bluetooth Hands-Free (HFP) capture endpoint
//! on Windows so apps cannot keep the headset microphone active and drain battery.

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct WindowsMicStatus {
    pub available: bool,
    pub enabled: bool,
    pub endpoint_name: Option<String>,
}

#[cfg(target_os = "windows")]
mod imp {
    use std::ffi::c_void;

    use windows::{
        Win32::{
            Foundation::{PROPERTYKEY, RPC_E_CHANGED_MODE},
            Media::Audio::{
                DEVICE_STATE, DEVICE_STATE_ACTIVE, DEVICE_STATE_DISABLED, IMMDevice,
                IMMDeviceEnumerator, MMDeviceEnumerator, eCapture,
            },
            System::Com::{
                CLSCTX_ALL, COINIT_MULTITHREADED, CoCreateInstance, CoInitializeEx, CoTaskMemFree,
                CoUninitialize, STGM_READ, StructuredStorage::PropVariantClear,
            },
        },
        core::{BOOL, GUID, HRESULT, Interface, PCWSTR, PWSTR},
    };

    use super::WindowsMicStatus;

    // PKEY_Device_FriendlyName = {a45c254e-df1c-4efd-8020-67d146a850e0}, 14
    const PKEY_DEVICE_FRIENDLY_NAME: PROPERTYKEY = PROPERTYKEY {
        fmtid: GUID::from_u128(0xa45c254e_df1c_4efd_8020_67d146a850e0),
        pid: 14,
    };

    // CPolicyConfigClient CLSID: {870af99c-171d-4f9e-af0d-e63df40c2bc9}
    const CLSID_POLICY_CONFIG_CLIENT: GUID =
        GUID::from_u128(0x870af99c_171d_4f9e_af0d_e63df40c2bc9);
    // IPolicyConfig IID: {f8679f50-850a-41cf-9c72-430f290290c8}
    const IID_POLICY_CONFIG: GUID = GUID::from_u128(0xf8679f50_850a_41cf_9c72_430f290290c8);
    // IPolicyConfigWin10 IID: {00000000-0000-0000-0000-000000000000} fallback -> IPolicyConfigVista {568b9108-44bf-40b4-9006-86afe5b5a620}
    const IID_POLICY_CONFIG_VISTA: GUID = GUID::from_u128(0x568b9108_44bf_40b4_9006_86afe5b5a620);

    #[repr(C)]
    struct PolicyConfigVtbl {
        // IUnknown (0..=2)
        query_interface:
            unsafe extern "system" fn(*mut c_void, *const GUID, *mut *mut c_void) -> HRESULT,
        add_ref: unsafe extern "system" fn(*mut c_void) -> u32,
        release: unsafe extern "system" fn(*mut c_void) -> u32,
        // IPolicyConfig (3..=12)
        _get_mix_format: *const c_void,
        _get_device_format: *const c_void,
        _reset_device_format: *const c_void,
        _set_device_format: *const c_void,
        _get_processing_period: *const c_void,
        _set_processing_period: *const c_void,
        _get_share_mode: *const c_void,
        _set_share_mode: *const c_void,
        _get_property_value: *const c_void,
        _set_property_value: *const c_void,
        // Slot 13: SetEndpointVisibility
        set_endpoint_visibility: unsafe extern "system" fn(*mut c_void, PCWSTR, BOOL) -> HRESULT,
    }

    #[repr(C)]
    struct RawComObject {
        vtbl: *const PolicyConfigVtbl,
    }

    struct ComGuard(bool);
    impl ComGuard {
        fn init() -> Result<Self, String> {
            let hr = unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) };
            if hr.is_ok() {
                Ok(Self(true))
            } else if hr == RPC_E_CHANGED_MODE {
                Ok(Self(false))
            } else {
                Err(format!("CoInitializeEx failed: {hr:?}"))
            }
        }
    }
    impl Drop for ComGuard {
        fn drop(&mut self) {
            if self.0 {
                unsafe { CoUninitialize() };
            }
        }
    }

    struct EndpointInfo {
        id_wide: Vec<u16>,
        friendly_name: String,
        is_active: bool,
    }

    unsafe fn pwstr_to_string_and_free(pwstr: PWSTR) -> (Vec<u16>, String) {
        if pwstr.is_null() {
            return (vec![0], String::new());
        }
        let mut len = 0usize;
        unsafe {
            while *pwstr.0.add(len) != 0 {
                len += 1;
            }
        }
        let slice = unsafe { std::slice::from_raw_parts(pwstr.0, len) };
        let s = String::from_utf16_lossy(slice);
        let mut wide = slice.to_vec();
        wide.push(0);
        unsafe { CoTaskMemFree(Some(pwstr.0 as *const c_void)) };
        (wide, s)
    }

    fn read_friendly_name(device: &IMMDevice) -> String {
        unsafe {
            let Ok(store) = device.OpenPropertyStore(STGM_READ) else {
                return String::new();
            };
            let Ok(mut prop) = store.GetValue(&PKEY_DEVICE_FRIENDLY_NAME) else {
                return String::new();
            };
            let name = prop.to_string();
            let _ = PropVariantClear(&mut prop);
            name
        }
    }

    fn build_match_tokens(device_name: &str, model_name: &str) -> Vec<String> {
        let mut tokens = Vec::new();
        let dn = device_name.trim().to_ascii_lowercase();
        if !dn.is_empty() {
            tokens.push(dn.clone());
            // Also match first two significant words (e.g., "airpods pro" from "AirPods Pro de Bernardo")
            let words: Vec<&str> = dn.split_whitespace().collect();
            if words.len() >= 2 {
                tokens.push(format!("{} {}", words[0], words[1]));
            }
        }
        let mn = model_name.trim().to_ascii_lowercase();
        if !mn.is_empty() {
            tokens.push(mn.clone());
            if mn.contains("airpods") {
                tokens.push("airpods".to_owned());
            }
            if mn.contains("q30") {
                tokens.push("q30".to_owned());
                tokens.push("soundcore".to_owned());
            }
        }
        tokens.sort();
        tokens.dedup();
        tokens
    }

    fn matches_tokens(friendly_name: &str, tokens: &[String]) -> bool {
        let lower = friendly_name.to_ascii_lowercase();
        tokens
            .iter()
            .any(|tok| !tok.is_empty() && lower.contains(tok))
    }

    fn enumerate_matching_capture_endpoints(
        device_name: &str,
        model_name: &str,
    ) -> Result<Vec<EndpointInfo>, String> {
        let _com = ComGuard::init()?;
        let tokens = build_match_tokens(device_name, model_name);
        if tokens.is_empty() {
            return Ok(Vec::new());
        }

        let enumerator: IMMDeviceEnumerator =
            unsafe { CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL) }
                .map_err(|e| format!("MMDeviceEnumerator failed: {e}"))?;

        let collection = unsafe {
            enumerator.EnumAudioEndpoints(
                eCapture,
                DEVICE_STATE(DEVICE_STATE_ACTIVE.0 | DEVICE_STATE_DISABLED.0),
            )
        }
        .map_err(|e| format!("EnumAudioEndpoints failed: {e}"))?;

        let count =
            unsafe { collection.GetCount() }.map_err(|e| format!("GetCount failed: {e}"))?;

        let mut results = Vec::new();
        for idx in 0..count {
            let Ok(device) = (unsafe { collection.Item(idx) }) else {
                continue;
            };
            let friendly_name = read_friendly_name(&device);
            if !matches_tokens(&friendly_name, &tokens) {
                continue;
            }
            let Ok(id_pwstr) = (unsafe { device.GetId() }) else {
                continue;
            };
            let (id_wide, _) = unsafe { pwstr_to_string_and_free(id_pwstr) };
            let state = unsafe { device.GetState() }.unwrap_or(DEVICE_STATE_DISABLED);
            results.push(EndpointInfo {
                id_wide,
                friendly_name,
                is_active: state == DEVICE_STATE_ACTIVE,
            });
        }

        Ok(results)
    }

    fn set_endpoint_visibility(id_wide: &[u16], visible: bool) -> Result<(), String> {
        let unknown: windows::core::IUnknown =
            unsafe { CoCreateInstance(&CLSID_POLICY_CONFIG_CLIENT, None, CLSCTX_ALL) }
                .map_err(|e| format!("CPolicyConfigClient failed: {e}"))?;

        let mut raw_ptr: *mut c_void = std::ptr::null_mut();
        let mut hr = unsafe {
            (Interface::vtable(&unknown).QueryInterface)(
                unknown.as_raw(),
                &IID_POLICY_CONFIG,
                &mut raw_ptr,
            )
        };
        if hr.is_err() || raw_ptr.is_null() {
            hr = unsafe {
                (Interface::vtable(&unknown).QueryInterface)(
                    unknown.as_raw(),
                    &IID_POLICY_CONFIG_VISTA,
                    &mut raw_ptr,
                )
            };
        }
        if hr.is_err() || raw_ptr.is_null() {
            return Err(format!("QueryInterface(IPolicyConfig) failed: {hr:?}"));
        }

        let obj = raw_ptr as *mut RawComObject;
        let vis_hr = unsafe {
            let vtbl = &*(*obj).vtbl;
            let res = (vtbl.set_endpoint_visibility)(
                raw_ptr,
                PCWSTR(id_wide.as_ptr()),
                BOOL::from(visible),
            );
            (vtbl.release)(raw_ptr);
            res
        };

        if vis_hr.is_ok() {
            Ok(())
        } else {
            Err(format!("SetEndpointVisibility failed: {vis_hr:?}"))
        }
    }

    pub fn query_device_mic(device_name: &str, model_name: &str) -> WindowsMicStatus {
        match enumerate_matching_capture_endpoints(device_name, model_name) {
            Ok(endpoints) if !endpoints.is_empty() => {
                let any_active = endpoints.iter().any(|e| e.is_active);
                let name = endpoints.into_iter().next().map(|e| e.friendly_name);
                WindowsMicStatus {
                    available: true,
                    enabled: any_active,
                    endpoint_name: name,
                }
            }
            _ => WindowsMicStatus::default(),
        }
    }

    pub fn set_device_mic_enabled(
        device_name: &str,
        model_name: &str,
        enabled: bool,
    ) -> Result<WindowsMicStatus, String> {
        let endpoints = enumerate_matching_capture_endpoints(device_name, model_name)?;
        if endpoints.is_empty() {
            return Err("No matching Windows microphone endpoint found".to_owned());
        }
        for ep in &endpoints {
            set_endpoint_visibility(&ep.id_wide, enabled)?;
        }
        Ok(query_device_mic(device_name, model_name))
    }
}

#[cfg(not(target_os = "windows"))]
mod imp {
    use super::WindowsMicStatus;

    pub fn query_device_mic(_device_name: &str, _model_name: &str) -> WindowsMicStatus {
        WindowsMicStatus::default()
    }

    pub fn set_device_mic_enabled(
        _device_name: &str,
        _model_name: &str,
        _enabled: bool,
    ) -> Result<WindowsMicStatus, String> {
        Err("Windows microphone control is only available on Windows".to_owned())
    }
}

pub use imp::{query_device_mic, set_device_mic_enabled};
