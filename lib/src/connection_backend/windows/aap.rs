use std::{
    collections::HashSet, io, mem::size_of, os::windows::io::AsRawHandle, panic::Location,
    sync::Arc, time::Duration,
};

use async_trait::async_trait;
use macaddr::MacAddr6;
use tokio::net::windows::named_pipe::{ClientOptions, NamedPipeClient, PipeMode};
use tracing::{debug, trace};
use windows::{
    Win32::{
        Devices::{
            Bluetooth::{BLUETOOTH_DEVICE_INFO, BluetoothGetDeviceInfo},
            DeviceAndDriverInstallation::{
                CM_GETIDLIST_FILTER_ENUMERATOR, CM_Get_DevNode_PropertyW,
                CM_Get_Device_ID_List_SizeW, CM_Get_Device_ID_ListW, CM_LOCATE_DEVNODE_PHANTOM,
                CM_Locate_DevNodeW, CR_BUFFER_SMALL, CR_SUCCESS,
            },
            Properties::{
                DEVPKEY_Device_FriendlyName, DEVPKEY_Device_HardwareIds, DEVPROP_TYPE_STRING,
                DEVPROP_TYPE_STRING_LIST, DEVPROPTYPE,
            },
        },
        Foundation::{
            DEVPROPKEY, ERROR_ACCESS_DENIED, ERROR_FILE_NOT_FOUND, ERROR_PIPE_BUSY, HANDLE,
        },
        System::{
            Pipes::GetNamedPipeServerProcessId,
            Services::{
                CloseServiceHandle, OpenSCManagerW, OpenServiceW, QueryServiceStatusEx, SC_HANDLE,
                SC_MANAGER_CONNECT, SC_STATUS_PROCESS_INFO, SERVICE_QUERY_STATUS, SERVICE_RUNNING,
                SERVICE_STATUS_PROCESS,
            },
        },
    },
    core::{PCWSTR, w},
};

use crate::{
    api::connection::{self, AapBackend, AapConnection, ConnectionDescriptor},
    connection_backend::wpods_aap::{
        MessagePipe, PIPE_READ_BUFFER_LEN, WPODS_AAP_PIPE_NAME, WPodsAapConnection,
    },
};

use super::utils::WindowsMacAddressExt;

const AAP_SERVICE_GUID_UPPER: &str = "{74EC2172-0BAD-4D01-8F77-997B2BE0722A}";
const APPLE_VID_PID_PREFIX_UPPER: &str = "VID&0001004C_PID&";
const SUPPORTED_APPLE_PIDS: &[u16] = &[0x2014, 0x2024];
const PIPE_BUSY_MAX_RETRIES: usize = 20;
const PIPE_BUSY_RETRY_DELAY: Duration = Duration::from_millis(25);
const ERROR_BROKEN_PIPE_RAW: i32 = 109;
const WPODS_STATUS_PIPE_NAME: &str = r"\\.\pipe\WPods.Service";
const WPODS_SETUP_URL: &str =
    "https://github.com/FPNDev/wpods/releases/download/v0.4.1/WPods-Setup.exe";
const WPODS_SETUP_SHA256: &str = "AE09E52052DEFBE164CFB2BED764A637AD44D5387B8329D410D0B0A96F6E9BE7";

#[derive(Default)]
pub struct WindowsAapBackend;

#[async_trait]
impl AapBackend for WindowsAapBackend {
    async fn devices(&self) -> connection::Result<HashSet<ConnectionDescriptor>> {
        tokio::task::spawn_blocking(discover_connected_airpods)
            .await
            .map_err(|err| connection::Error::Other {
                source: Box::new(err),
                location: Location::caller(),
            })?
    }

    async fn connect(
        &self,
        mac_address: MacAddr6,
    ) -> connection::Result<Arc<dyn AapConnection + Send + Sync>> {
        if open_wpods_pipe(WPODS_AAP_PIPE_NAME, true).await.is_err() {
            ensure_wpods_service_ready().await?;
        }

        let mut last_err: Option<connection::Error> = None;
        for attempt in 0..8 {
            let pipe = match open_wpods_pipe(WPODS_AAP_PIPE_NAME, true).await {
                Ok(pipe) => pipe,
                Err(err) => {
                    last_err = Some(err);
                    tokio::time::sleep(Duration::from_millis(800)).await;
                    continue;
                }
            };

            match WPodsAapConnection::open_with_pipe(Arc::new(pipe), mac_address).await {
                Ok(conn) => return Ok(Arc::new(conn)),
                Err(connection::Error::TransportError { code: 170, .. }) if attempt < 4 => {
                    // WPods.exe tray UI is holding the single AAP channel open; close the tray client
                    // while keeping WPodsService running so AirCore can claim the channel.
                    debug!("WPods.Aap returned ERROR_BUSY (170); closing WPods.exe tray client");
                    let _ = tokio::task::spawn_blocking(close_wpods_tray_client).await;
                    tokio::time::sleep(Duration::from_millis(600)).await;
                }
                Err(
                    err @ connection::Error::TransportError {
                        code: 1167 | 121, ..
                    },
                ) if attempt < 7 => {
                    // Adapter/ACL link may still be re-attaching right after WPodsService starts.
                    debug!(
                        "AirPods ACL link not ready yet (attempt {}); nudging WPods.Service and retrying",
                        attempt + 1
                    );
                    let _ = nudge_wpods_service_retry().await;
                    last_err = Some(err);
                    tokio::time::sleep(Duration::from_millis(1500)).await;
                }
                Err(err) => return Err(err),
            }
        }

        Err(
            last_err.unwrap_or_else(|| connection::Error::ServiceUnavailable {
                reason: "Timed out waiting for WPodsService AAP channel to become ready."
                    .to_owned(),
                source: None,
                location: Location::caller(),
            }),
        )
    }
}

fn is_wpods_service_registered() -> bool {
    let Ok(scm) = (unsafe { OpenSCManagerW(None, None, SC_MANAGER_CONNECT) }) else {
        return false;
    };
    let _scm = ScHandleGuard(scm);
    if let Ok(svc) = unsafe { OpenServiceW(_scm.0, w!("WPodsService"), SERVICE_QUERY_STATUS) } {
        let _svc = ScHandleGuard(svc);
        true
    } else {
        false
    }
}

fn close_wpods_tray_client() {
    use std::os::windows::process::CommandExt;
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    let _ = std::process::Command::new("taskkill.exe")
        .args(["/F", "/IM", "WPods.exe"])
        .creation_flags(CREATE_NO_WINDOW)
        .status();
}

async fn nudge_wpods_service_retry() -> io::Result<()> {
    let pipe = ClientOptions::new()
        .pipe_mode(PipeMode::Message)
        .open(WPODS_STATUS_PIPE_NAME)?;
    let msg_pipe = WindowsNamedPipe::new(pipe);
    // Protocol.Retry = 0x21 in WPods.Service
    msg_pipe.write_message(&[0x21]).await
}

async fn ensure_wpods_service_ready() -> connection::Result<()> {
    use std::os::windows::process::CommandExt;
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;

    let registered = tokio::task::spawn_blocking(is_wpods_service_registered)
        .await
        .unwrap_or(false);

    if registered {
        debug!("WPodsService is installed but not running; requesting service start");
        let _ = tokio::task::spawn_blocking(move || {
            std::process::Command::new("powershell.exe")
                .args([
                    "-NoProfile",
                    "-NonInteractive",
                    "-WindowStyle",
                    "Hidden",
                    "-Command",
                    "Start-Process sc.exe -ArgumentList 'start','WPodsService' -Verb RunAs -WindowStyle Hidden -Wait",
                ])
                .creation_flags(CREATE_NO_WINDOW)
                .status()
        })
        .await;

        for _ in 0..24 {
            if open_wpods_pipe(WPODS_AAP_PIPE_NAME, true).await.is_ok() {
                return Ok(());
            }
            tokio::time::sleep(Duration::from_millis(500)).await;
        }
    } else {
        debug!("WPodsService is not installed; downloading/launching official WPods-Setup.exe");
        let script = format!(
            "$ErrorActionPreference = 'Stop'; \
             $dest = Join-Path $env:TEMP 'WPods-Setup.exe'; \
             $expected = '{WPODS_SETUP_SHA256}'; \
             if (-not (Test-Path $dest) -or ((Get-FileHash $dest -Algorithm SHA256).Hash -ne $expected)) {{ \
                 Invoke-WebRequest -Uri '{WPODS_SETUP_URL}' -OutFile $dest -UseBasicParsing; \
             }}; \
             if ((Get-FileHash $dest -Algorithm SHA256).Hash -ne $expected) {{ throw 'SHA256 mismatch' }}; \
             Start-Process -FilePath $dest -ArgumentList '/SP-','/SILENT','/NORESTART','/TASKS=\"!startup,!desktopicon\"' -Wait"
        );
        let install_res = tokio::task::spawn_blocking(move || {
            std::process::Command::new("powershell.exe")
                .args([
                    "-NoProfile",
                    "-NonInteractive",
                    "-WindowStyle",
                    "Hidden",
                    "-Command",
                    &script,
                ])
                .creation_flags(CREATE_NO_WINDOW)
                .status()
        })
        .await;

        if matches!(install_res, Ok(Ok(status)) if status.success()) {
            let _ = tokio::task::spawn_blocking(close_wpods_tray_client).await;
            for _ in 0..30 {
                if open_wpods_pipe(WPODS_AAP_PIPE_NAME, true).await.is_ok() {
                    return Ok(());
                }
                tokio::time::sleep(Duration::from_millis(500)).await;
            }
        }
    }

    open_wpods_pipe(WPODS_AAP_PIPE_NAME, true).await.map(|_| ())
}

fn is_aap_devnode_id(id: &str) -> bool {
    let upper = id.to_ascii_uppercase();
    upper.contains("BTHENUM\\") && upper.contains(AAP_SERVICE_GUID_UPPER)
}

fn extract_apple_pid(text: &str) -> Option<u16> {
    let upper = text.to_ascii_uppercase();
    let idx = upper.find(APPLE_VID_PID_PREFIX_UPPER)?;
    let hex_start = idx + APPLE_VID_PID_PREFIX_UPPER.len();
    let hex = upper.get(hex_start..hex_start + 4)?;
    if !hex.bytes().all(|b| b.is_ascii_hexdigit()) {
        return None;
    }
    u16::from_str_radix(hex, 16).ok()
}

fn extract_mac_from_bthenum_instance_id(instance_id: &str) -> Option<MacAddr6> {
    let last_segment = instance_id.rsplit('\\').next()?;
    let without_suffix = last_segment.split('_').next()?;
    let mac_hex = without_suffix.rsplit('&').next()?;
    if mac_hex.len() != 12 || !mac_hex.bytes().all(|b| b.is_ascii_hexdigit()) {
        return None;
    }
    let mut bytes = [0_u8; 6];
    for (i, slot) in bytes.iter_mut().enumerate() {
        let start = i * 2;
        *slot = u8::from_str_radix(&mac_hex[start..start + 2], 16).ok()?;
    }
    Some(MacAddr6::from(bytes))
}

fn decode_multi_sz_utf16(buf: &[u16]) -> Vec<String> {
    let mut result = Vec::new();
    let mut start = 0;
    while start < buf.len() {
        let end = buf[start..]
            .iter()
            .position(|&c| c == 0)
            .map_or(buf.len(), |pos| start + pos);
        if end == start {
            break;
        }
        result.push(String::from_utf16_lossy(&buf[start..end]));
        start = end + 1;
    }
    result
}

fn read_devnode_string_property(devinst: u32, key: &DEVPROPKEY) -> Option<Vec<String>> {
    let mut prop_type = DEVPROPTYPE(0);
    let mut prop_size = 0_u32;
    let status =
        unsafe { CM_Get_DevNode_PropertyW(devinst, key, &mut prop_type, None, &mut prop_size, 0) };
    if status != CR_BUFFER_SMALL || prop_size == 0 {
        return None;
    }
    let mut bytes = vec![0_u8; prop_size as usize];
    let status = unsafe {
        CM_Get_DevNode_PropertyW(
            devinst,
            key,
            &mut prop_type,
            Some(bytes.as_mut_ptr()),
            &mut prop_size,
            0,
        )
    };
    if status != CR_SUCCESS {
        return None;
    }
    if prop_type != DEVPROP_TYPE_STRING && prop_type != DEVPROP_TYPE_STRING_LIST {
        return None;
    }
    let valid_len = (prop_size as usize).min(bytes.len());
    let (chunks, _) = bytes[..valid_len].as_chunks::<2>();
    let wide: Vec<u16> = chunks
        .iter()
        .map(|chunk| u16::from_le_bytes(*chunk))
        .collect();
    if prop_type == DEVPROP_TYPE_STRING_LIST {
        Some(decode_multi_sz_utf16(&wide))
    } else {
        let nul = wide.iter().position(|&c| c == 0).unwrap_or(wide.len());
        let s = String::from_utf16_lossy(&wide[..nul]);
        if s.is_empty() { None } else { Some(vec![s]) }
    }
}

fn enumerate_bthenum_device_ids() -> connection::Result<Vec<String>> {
    for _ in 0..4 {
        let mut len = 0_u32;
        let status = unsafe {
            CM_Get_Device_ID_List_SizeW(&mut len, w!("BTHENUM"), CM_GETIDLIST_FILTER_ENUMERATOR)
        };
        if status != CR_SUCCESS {
            return Err(connection::Error::Other {
                source: io::Error::other(format!(
                    "CM_Get_Device_ID_List_SizeW failed with CONFIGRET {}",
                    status.0
                ))
                .into(),
                location: Location::caller(),
            });
        }
        if len == 0 {
            return Ok(Vec::new());
        }
        let mut buf = vec![0_u16; len as usize];
        let status = unsafe {
            CM_Get_Device_ID_ListW(w!("BTHENUM"), &mut buf, CM_GETIDLIST_FILTER_ENUMERATOR)
        };
        if status == CR_SUCCESS {
            return Ok(decode_multi_sz_utf16(&buf));
        }
        if status != CR_BUFFER_SMALL {
            return Err(connection::Error::Other {
                source: io::Error::other(format!(
                    "CM_Get_Device_ID_ListW failed with CONFIGRET {}",
                    status.0
                ))
                .into(),
                location: Location::caller(),
            });
        }
    }
    Ok(Vec::new())
}

fn discover_connected_airpods() -> connection::Result<HashSet<ConnectionDescriptor>> {
    let device_ids = enumerate_bthenum_device_ids()?;
    let mut descriptors = HashSet::new();

    for id in device_ids {
        if !is_aap_devnode_id(&id) {
            continue;
        }
        let Some(mac_address) = extract_mac_from_bthenum_instance_id(&id) else {
            trace!("skipping AAP devnode without parseable MAC: {id}");
            continue;
        };

        let mut wide_id: Vec<u16> = id.encode_utf16().chain(std::iter::once(0)).collect();
        let mut devinst = 0_u32;
        let status = unsafe {
            CM_Locate_DevNodeW(
                &mut devinst,
                PCWSTR(wide_id.as_mut_ptr()),
                CM_LOCATE_DEVNODE_PHANTOM,
            )
        };
        if status != CR_SUCCESS {
            continue;
        }

        let pid = extract_apple_pid(&id).or_else(|| {
            read_devnode_string_property(devinst, &DEVPKEY_Device_HardwareIds)?
                .into_iter()
                .find_map(|hw_id| extract_apple_pid(&hw_id))
        });
        let Some(pid) = pid else {
            trace!("skipping AAP devnode without Apple VID/PID: {id}");
            continue;
        };
        if !SUPPORTED_APPLE_PIDS.contains(&pid) {
            trace!("skipping unsupported Apple PID 0x{pid:04X} on devnode {id}");
            continue;
        }

        let mut bt_info = BLUETOOTH_DEVICE_INFO {
            dwSize: size_of::<BLUETOOTH_DEVICE_INFO>() as u32,
            ..Default::default()
        };
        bt_info.Address.Anonymous.ullLong = mac_address.as_windows_u64();

        let ret = unsafe { BluetoothGetDeviceInfo(None, &mut bt_info) };
        if ret != 0 || !bt_info.fConnected.as_bool() {
            debug!("paired AirPods {mac_address} (PID 0x{pid:04X}) not connected (ret={ret})");
            continue;
        }

        let nul_pos = bt_info
            .szName
            .iter()
            .position(|&c| c == 0)
            .unwrap_or(bt_info.szName.len());
        let bt_name = String::from_utf16_lossy(&bt_info.szName[..nul_pos])
            .trim()
            .to_owned();
        let name = if !bt_name.is_empty() {
            bt_name
        } else {
            read_devnode_string_property(devinst, &DEVPKEY_Device_FriendlyName)
                .and_then(|mut v| v.pop())
                .filter(|s| !s.trim().is_empty())
                .unwrap_or_else(|| "Apple AirPods Pro 2".to_owned())
        };

        descriptors.insert(ConnectionDescriptor { name, mac_address });
    }

    Ok(descriptors)
}

struct ScHandleGuard(SC_HANDLE);

impl Drop for ScHandleGuard {
    fn drop(&mut self) {
        if !self.0.is_invalid() {
            let _ = unsafe { CloseServiceHandle(self.0) };
        }
    }
}

fn map_pipe_open_error(err: io::Error) -> connection::Error {
    match err.raw_os_error().map(|c| c as u32) {
        Some(code) if code == ERROR_FILE_NOT_FOUND.0 => connection::Error::ServiceUnavailable {
            reason: "WPods.Service is not running or not installed (\\\\.\\pipe\\WPods.Aap not found). Start WPodsService for real AirPods Pro 2 hardware, or use Demo Mode.".to_owned(),
            source: Some(Box::new(err)),
            location: Location::caller(),
        },
        Some(code) if code == ERROR_ACCESS_DENIED.0 => connection::Error::ServiceUnavailable {
            reason: "Access denied opening \\\\.\\pipe\\WPods.Aap (check WPodsService permissions).".to_owned(),
            source: Some(Box::new(err)),
            location: Location::caller(),
        },
        _ => connection::Error::Other {
            source: Box::new(err),
            location: Location::caller(),
        },
    }
}

fn verify_wpods_service_pid(server_pid: u32) -> connection::Result<()> {
    let scm = ScHandleGuard(
        unsafe { OpenSCManagerW(None, None, SC_MANAGER_CONNECT) }.map_err(|err| {
            connection::Error::ServiceUnavailable {
                reason: format!(
                    "Failed to open Service Control Manager to verify WPodsService: {err}"
                ),
                source: Some(Box::new(err)),
                location: Location::caller(),
            }
        })?,
    );

    let service = ScHandleGuard(
        unsafe { OpenServiceW(scm.0, w!("WPodsService"), SERVICE_QUERY_STATUS) }.map_err(
            |err| connection::Error::ServiceUnavailable {
                reason: format!(
                    "WPodsService is not registered in Windows Service Control Manager: {err}"
                ),
                source: Some(Box::new(err)),
                location: Location::caller(),
            },
        )?,
    );

    let mut status_bytes = [0_u8; size_of::<SERVICE_STATUS_PROCESS>()];
    let mut bytes_needed = 0_u32;
    unsafe {
        QueryServiceStatusEx(
            service.0,
            SC_STATUS_PROCESS_INFO,
            Some(&mut status_bytes),
            &mut bytes_needed,
        )
    }
    .map_err(|err| connection::Error::ServiceUnavailable {
        reason: format!("Failed to query WPodsService status: {err}"),
        source: Some(Box::new(err)),
        location: Location::caller(),
    })?;

    let status: SERVICE_STATUS_PROCESS =
        unsafe { std::ptr::read_unaligned(status_bytes.as_ptr().cast()) };
    if status.dwCurrentState != SERVICE_RUNNING || status.dwProcessId != server_pid {
        return Err(connection::Error::ServiceUnavailable {
            reason: format!(
                "Pipe \\\\.\\pipe\\WPods.Aap server PID ({server_pid}) does not match running WPodsService PID ({})",
                status.dwProcessId
            ),
            source: None,
            location: Location::caller(),
        });
    }

    Ok(())
}

async fn open_wpods_pipe(
    pipe_name: &str,
    verify_service: bool,
) -> connection::Result<WindowsNamedPipe> {
    let mut attempt = 0;
    let client = loop {
        match ClientOptions::new()
            .pipe_mode(PipeMode::Message)
            .open(pipe_name)
        {
            Ok(client) => break client,
            Err(err)
                if err.raw_os_error().map(|c| c as u32) == Some(ERROR_PIPE_BUSY.0)
                    && attempt < PIPE_BUSY_MAX_RETRIES =>
            {
                attempt += 1;
                tokio::time::sleep(PIPE_BUSY_RETRY_DELAY).await;
            }
            Err(err) => return Err(map_pipe_open_error(err)),
        }
    };

    if verify_service {
        let mut server_pid = 0_u32;
        unsafe { GetNamedPipeServerProcessId(HANDLE(client.as_raw_handle()), &mut server_pid) }
            .map_err(|err| connection::Error::ServiceUnavailable {
                reason: format!("Failed to verify WPods.Aap pipe server process: {err}"),
                source: Some(Box::new(err)),
                location: Location::caller(),
            })?;
        tokio::task::spawn_blocking(move || verify_wpods_service_pid(server_pid))
            .await
            .map_err(|err| connection::Error::Other {
                source: Box::new(err),
                location: Location::caller(),
            })??;
    }

    Ok(WindowsNamedPipe::new(client))
}

struct WindowsNamedPipe {
    client: NamedPipeClient,
}

impl WindowsNamedPipe {
    fn new(client: NamedPipeClient) -> Self {
        Self { client }
    }
}

#[async_trait]
impl MessagePipe for WindowsNamedPipe {
    async fn read_message(&self) -> io::Result<Option<Vec<u8>>> {
        let mut buf = vec![0_u8; PIPE_READ_BUFFER_LEN];
        loop {
            self.client.readable().await?;
            match self.client.try_read(&mut buf) {
                Ok(0) => return Ok(None),
                Ok(n) => return Ok(Some(buf[..n].to_vec())),
                Err(err) if err.kind() == io::ErrorKind::WouldBlock => continue,
                Err(err) if err.raw_os_error() == Some(ERROR_BROKEN_PIPE_RAW) => return Ok(None),
                Err(err) => return Err(err),
            }
        }
    }

    async fn write_message(&self, msg: &[u8]) -> io::Result<()> {
        loop {
            self.client.writable().await?;
            match self.client.try_write(msg) {
                Ok(0) if !msg.is_empty() => {
                    return Err(io::Error::new(
                        io::ErrorKind::WriteZero,
                        "wrote zero bytes to named pipe",
                    ));
                }
                Ok(_) => return Ok(()),
                Err(err) if err.kind() == io::ErrorKind::WouldBlock => continue,
                Err(err) => return Err(err),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::{
        str::FromStr,
        sync::atomic::{AtomicUsize, Ordering},
    };

    use tokio::net::windows::named_pipe::{
        NamedPipeServer, PipeMode as ServerPipeMode, ServerOptions,
    };

    use crate::api::connection::ConnectionStatus;

    use super::*;

    static NEXT_PIPE_ID: AtomicUsize = AtomicUsize::new(1);

    fn unique_pipe_name(prefix: &str) -> String {
        let id = NEXT_PIPE_ID.fetch_add(1, Ordering::Relaxed);
        format!(r"\\.\pipe\{prefix}-{}-{id}", std::process::id())
    }

    async fn server_read_msg(server: &NamedPipeServer) -> Vec<u8> {
        let mut buf = vec![0_u8; 4096];
        loop {
            server.readable().await.unwrap();
            match server.try_read(&mut buf) {
                Ok(n) => return buf[..n].to_vec(),
                Err(e) if e.kind() == io::ErrorKind::WouldBlock => continue,
                Err(e) => panic!("pipe server read failed: {e}"),
            }
        }
    }

    async fn server_write_msg(server: &NamedPipeServer, data: &[u8]) {
        loop {
            server.writable().await.unwrap();
            match server.try_write(data) {
                Ok(_) => return,
                Err(e) if e.kind() == io::ErrorKind::WouldBlock => continue,
                Err(e) => panic!("pipe server write failed: {e}"),
            }
        }
    }

    #[test]
    fn parses_bthenum_aap_devnodes_pids_and_macs() {
        let id_usbc = r"BTHENUM\{74EC2172-0BAD-4D01-8F77-997B2BE0722A}_VID&0001004C_PID&2024\8&1A2B3C4D&0&AABBCCDDEEFF_C00000000";
        let id_lightning = r"bthenum\{74ec2172-0bad-4d01-8f77-997b2be0722a}_vid&0001004c_pid&2014\7&deadbeef&0&112233445566";
        let id_other = r"BTHENUM\{0000110B-0000-1000-8000-00805F9B34FB}_VID&0001004C_PID&2024\8&1A2B3C4D&0&AABBCCDDEEFF_C00000000";

        assert!(is_aap_devnode_id(id_usbc));
        assert!(is_aap_devnode_id(id_lightning));
        assert!(!is_aap_devnode_id(id_other));

        assert_eq!(extract_apple_pid(id_usbc), Some(0x2024));
        assert_eq!(extract_apple_pid(id_lightning), Some(0x2014));
        assert_eq!(extract_apple_pid("VID&00020000_PID&1234"), None);

        assert_eq!(
            extract_mac_from_bthenum_instance_id(id_usbc),
            Some(MacAddr6::from_str("AA:BB:CC:DD:EE:FF").unwrap())
        );
        assert_eq!(
            extract_mac_from_bthenum_instance_id(id_lightning),
            Some(MacAddr6::from_str("11:22:33:44:55:66").unwrap())
        );
        assert_eq!(
            extract_mac_from_bthenum_instance_id("BTHENUM\\invalid"),
            None
        );
    }

    #[tokio::test]
    async fn missing_pipe_returns_actionable_service_unavailable() {
        let missing = unique_pipe_name("openscq30-missing");
        let err = open_wpods_pipe(&missing, false).await.err().unwrap();
        match err {
            connection::Error::ServiceUnavailable { reason, .. } => {
                assert!(reason.contains("WPods.Service"));
                assert!(reason.contains("Demo Mode"));
            }
            other => panic!("expected ServiceUnavailable, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn in_process_named_pipe_roundtrip() {
        let pipe_name = unique_pipe_name("openscq30-test-aap");
        let server = ServerOptions::new()
            .first_pipe_instance(true)
            .pipe_mode(ServerPipeMode::Message)
            .create(&pipe_name)
            .unwrap();

        let mac = MacAddr6::from_str("01:23:45:67:89:AB").unwrap();

        let server_task = tokio::spawn(async move {
            server.connect().await.unwrap();

            // Read 10-byte "AP" + LE u64 MAC
            let open_buf = server_read_msg(&server).await;
            assert_eq!(open_buf.len(), 10);
            assert_eq!(&open_buf[..2], b"AP");

            // Reply with 0x02 + 0i32
            server_write_msg(&server, &[0x02, 0, 0, 0, 0]).await;

            // Send HFP call audio active (0x03 0x01)
            server_write_msg(&server, &[0x03, 0x01]).await;

            // Read outbound AAP handshake from client
            let pkt_buf = server_read_msg(&server).await;
            assert_eq!(pkt_buf, vec![0x00, 0x00, 0x04, 0x00, 0x01, 0x00]);

            // Echo inbound AAP packet (0x00 + payload)
            server_write_msg(&server, &[0x00, 0x04, 0x00, 0x04, 0x00, 0x1D, 0x00]).await;

            // Close channel (0x01 + 0i32)
            server_write_msg(&server, &[0x01, 0, 0, 0, 0]).await;
        });

        let pipe = open_wpods_pipe(&pipe_name, false).await.unwrap();
        let conn = WPodsAapConnection::open_with_pipe(Arc::new(pipe), mac)
            .await
            .unwrap();

        let mut rx = conn.read_channel();
        let mut status_rx = conn.connection_status();
        let mut call_rx = conn.call_audio_active().unwrap();

        assert_eq!(*status_rx.borrow_and_update(), ConnectionStatus::Connected);

        conn.write(&[0x00, 0x00, 0x04, 0x00, 0x01, 0x00])
            .await
            .unwrap();

        call_rx.changed().await.unwrap();
        assert!(*call_rx.borrow_and_update());

        let inbound = rx.recv().await.unwrap();
        assert_eq!(inbound, vec![0x04, 0x00, 0x04, 0x00, 0x1D, 0x00]);

        status_rx.changed().await.unwrap();
        assert_eq!(
            *status_rx.borrow_and_update(),
            ConnectionStatus::Disconnected
        );
        server_task.await.unwrap();
    }
}
