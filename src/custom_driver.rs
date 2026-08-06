use serde::Deserialize;
use std::path::{Component, Path, PathBuf};
use std::sync::atomic::{AtomicPtr, Ordering};

const DRIVER_META_FILE: &str = "meta.json";
const MAX_DRIVER_FILES: usize = 1024;
const MAX_DRIVER_SIZE: u64 = 512 * 1024 * 1024;

static VK_GET_INSTANCE_PROC_ADDR: AtomicPtr<std::ffi::c_void> =
    AtomicPtr::new(std::ptr::null_mut());

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DriverMetadata {
    pub name: String,
    #[serde(default)]
    pub author: String,
    #[serde(default)]
    pub package_version: String,
    #[serde(default)]
    pub driver_version: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub min_api: u32,
    pub library_name: String,
}

#[derive(Clone, Debug)]
pub struct InstalledDriver {
    pub id: String,
    pub metadata: DriverMetadata,
    #[cfg(target_arch = "aarch64")]
    pub directory: PathBuf,
}

pub fn drivers_directory() -> PathBuf {
    crate::ui::get_dirs().config_dir.join("gpu_drivers")
}

pub fn supports_custom_drivers() -> bool {
    cfg!(all(target_os = "android", target_arch = "aarch64")) && Path::new("/dev/kgsl-3d0").exists()
}

pub fn list_installed() -> Vec<InstalledDriver> {
    let Ok(entries) = std::fs::read_dir(drivers_directory()) else {
        return Vec::new();
    };

    let mut drivers: Vec<_> = entries
        .filter_map(Result::ok)
        .filter_map(|entry| {
            let directory = entry.path();
            if !directory.is_dir() {
                return None;
            }
            let metadata = read_metadata(&directory).ok()?;
            let library = safe_relative_path(&metadata.library_name)?;
            if !directory.join(library).is_file() {
                return None;
            }
            Some(InstalledDriver {
                id: entry.file_name().to_string_lossy().into_owned(),
                metadata,
                #[cfg(target_arch = "aarch64")]
                directory,
            })
        })
        .collect();
    drivers.sort_by_key(|driver| driver.metadata.name.to_lowercase());
    drivers
}

pub fn install<R: std::io::Read + std::io::Seek>(reader: R) -> Result<String, String> {
    let root = drivers_directory();
    std::fs::create_dir_all(&root).map_err(|error| error.to_string())?;

    let temp = root.join(format!(".installing-{}", std::process::id()));
    if temp.exists() {
        std::fs::remove_dir_all(&temp).map_err(|error| error.to_string())?;
    }
    std::fs::create_dir(&temp).map_err(|error| error.to_string())?;

    let result = extract_and_validate(reader, &temp).and_then(|metadata| {
        let id = unique_driver_id(&root, &metadata.name);
        std::fs::rename(&temp, root.join(&id)).map_err(|error| error.to_string())?;
        Ok(id)
    });

    if result.is_err() {
        let _ = std::fs::remove_dir_all(&temp);
    }
    result
}

pub fn remove(driver_id: &str) -> Result<(), String> {
    let Some(id) = safe_driver_id(driver_id) else {
        return Err("Invalid driver identifier".into());
    };
    let path = drivers_directory().join(id);
    if path.is_dir() {
        std::fs::remove_dir_all(path).map_err(|error| error.to_string())?;
    }
    Ok(())
}

fn extract_and_validate<R: std::io::Read + std::io::Seek>(
    reader: R,
    destination: &Path,
) -> Result<DriverMetadata, String> {
    let mut archive = zip::ZipArchive::new(reader).map_err(|error| error.to_string())?;
    if archive.len() > MAX_DRIVER_FILES {
        return Err("Driver archive contains too many files".into());
    }

    let mut total_size = 0_u64;
    for index in 0..archive.len() {
        let mut entry = archive.by_index(index).map_err(|error| error.to_string())?;
        total_size = total_size
            .checked_add(entry.size())
            .filter(|size| *size <= MAX_DRIVER_SIZE)
            .ok_or_else(|| "Driver archive is too large".to_string())?;

        if entry
            .unix_mode()
            .is_some_and(|mode| mode & 0o170000 == 0o120000)
        {
            return Err("Driver archive contains a symbolic link".into());
        }
        let Some(relative_path) = entry.enclosed_name() else {
            return Err("Driver archive contains an unsafe path".into());
        };
        let output_path = destination.join(&relative_path);
        if entry.is_dir() {
            std::fs::create_dir_all(&output_path).map_err(|error| error.to_string())?;
            continue;
        }
        if let Some(parent) = output_path.parent() {
            std::fs::create_dir_all(parent).map_err(|error| error.to_string())?;
        }
        let mut output = std::fs::File::create(output_path).map_err(|error| error.to_string())?;
        std::io::copy(&mut entry, &mut output).map_err(|error| error.to_string())?;
    }

    let metadata = read_metadata(destination)?;
    if metadata.name.trim().is_empty() {
        return Err("Driver metadata does not contain a name".into());
    }
    let library = safe_relative_path(&metadata.library_name)
        .ok_or_else(|| "Driver metadata contains an invalid library name".to_string())?;
    if !destination.join(library).is_file() {
        return Err(format!(
            "Driver library '{}' is missing",
            metadata.library_name
        ));
    }
    if metadata.min_api > android_api_level() {
        return Err(format!(
            "Driver requires Android API {}, but this device uses API {}",
            metadata.min_api,
            android_api_level()
        ));
    }
    Ok(metadata)
}

fn read_metadata(directory: &Path) -> Result<DriverMetadata, String> {
    let data = std::fs::read(directory.join(DRIVER_META_FILE))
        .map_err(|_| "Driver archive does not contain meta.json at its root".to_string())?;
    serde_json::from_slice(&data).map_err(|error| format!("Invalid driver metadata: {error}"))
}

fn safe_relative_path(path: &str) -> Option<&Path> {
    let path = Path::new(path);
    if path.as_os_str().is_empty()
        || path
            .components()
            .any(|component| !matches!(component, Component::Normal(_)))
    {
        None
    } else {
        Some(path)
    }
}

fn safe_driver_id(id: &str) -> Option<&str> {
    (!id.is_empty()
        && !matches!(id, "." | "..")
        && id
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.')))
    .then_some(id)
}

fn unique_driver_id(root: &Path, name: &str) -> String {
    let base: String = name
        .chars()
        .map(|character| {
            if character.is_ascii_alphanumeric() || matches!(character, '-' | '_' | '.') {
                character
            } else {
                '-'
            }
        })
        .collect::<String>()
        .trim_matches(|character| matches!(character, '-' | '.'))
        .chars()
        .take(80)
        .collect();
    let base = if base.is_empty() { "driver" } else { &base };
    if !root.join(base).exists() {
        return base.to_string();
    }
    for suffix in 2.. {
        let candidate = format!("{base}-{suffix}");
        if !root.join(&candidate).exists() {
            return candidate;
        }
    }
    unreachable!()
}

#[cfg(target_os = "android")]
fn android_api_level() -> u32 {
    #[link(name = "android")]
    unsafe extern "C" {
        fn android_get_device_api_level() -> std::ffi::c_int;
    }
    unsafe { android_get_device_api_level().max(0) as u32 }
}

#[cfg(not(target_os = "android"))]
fn android_api_level() -> u32 {
    0
}

#[cfg(all(target_os = "android", target_arch = "aarch64"))]
pub fn load_selected(driver_id: &str, native_library_dir: Option<&str>) {
    VK_GET_INSTANCE_PROC_ADDR.store(std::ptr::null_mut(), Ordering::Release);
    if driver_id.is_empty() {
        return;
    }
    if !supports_custom_drivers() {
        eprintln!("Custom GPU driver is selected, but this device does not support it");
        return;
    }

    let Some(driver) = list_installed()
        .into_iter()
        .find(|driver| driver.id == driver_id)
    else {
        eprintln!("Selected custom GPU driver is not installed: {driver_id}");
        return;
    };
    let Some(native_library_dir) = native_library_dir else {
        eprintln!("Could not determine the Android native library directory");
        return;
    };

    match unsafe { open_driver(&driver, native_library_dir) } {
        Ok(proc_addr) => {
            VK_GET_INSTANCE_PROC_ADDR.store(proc_addr, Ordering::Release);
            eprintln!("Loaded custom GPU driver: {}", driver.metadata.name);
        }
        Err(error) => {
            eprintln!(
                "Could not load custom GPU driver '{}': {error}",
                driver.metadata.name
            );
            eprintln!("Falling back to the system Vulkan driver");
        }
    }
}

#[cfg(not(all(target_os = "android", target_arch = "aarch64")))]
pub fn load_selected(_driver_id: &str, _native_library_dir: Option<&str>) {}

pub fn vk_get_instance_proc_addr() -> *mut std::ffi::c_void {
    VK_GET_INSTANCE_PROC_ADDR.load(Ordering::Acquire)
}

#[cfg(all(target_os = "android", target_arch = "aarch64"))]
unsafe fn open_driver(
    driver: &InstalledDriver,
    native_library_dir: &str,
) -> Result<*mut std::ffi::c_void, String> {
    type OpenLibvulkan = unsafe extern "C" fn(
        std::ffi::c_int,
        std::ffi::c_int,
        *const std::ffi::c_char,
        *const std::ffi::c_char,
        *const std::ffi::c_char,
        *const std::ffi::c_char,
        *const std::ffi::c_char,
        *mut *mut std::ffi::c_void,
    ) -> *mut std::ffi::c_void;

    unsafe extern "C" {
        fn dlopen(
            filename: *const std::ffi::c_char,
            flags: std::ffi::c_int,
        ) -> *mut std::ffi::c_void;
        fn dlsym(
            handle: *mut std::ffi::c_void,
            symbol: *const std::ffi::c_char,
        ) -> *mut std::ffi::c_void;
    }

    let library = std::ffi::CString::new("libadrenotools.so").unwrap();
    let adrenotools = unsafe { dlopen(library.as_ptr(), 2) };
    if adrenotools.is_null() {
        return Err("libadrenotools.so could not be opened".into());
    }
    let symbol = std::ffi::CString::new("adrenotools_open_libvulkan").unwrap();
    let open_symbol = unsafe { dlsym(adrenotools, symbol.as_ptr()) };
    if open_symbol.is_null() {
        return Err("adrenotools_open_libvulkan was not found".into());
    }
    let open: OpenLibvulkan = unsafe { std::mem::transmute(open_symbol) };

    let redirect_directory = driver.directory.join("file_redirect");
    std::fs::create_dir_all(&redirect_directory).map_err(|error| error.to_string())?;
    let hooks = path_to_c_string(Path::new(native_library_dir))?;
    // AdrenoTools expects the driver directory to end with a separator.
    let driver_directory = driver.directory.join("");
    let directory = path_to_c_string(&driver_directory)?;
    let library_name = std::ffi::CString::new(driver.metadata.library_name.as_str())
        .map_err(|error| error.to_string())?;
    let redirect = path_to_c_string(&redirect_directory)?;

    // AdrenoTools and the selected Vulkan implementation intentionally remain
    // loaded for the lifetime of the application. Vulkan objects cannot outlive
    // either shared library, so changing drivers requires restarting Gopher64.
    let vulkan = unsafe {
        open(
            2,
            1 | 2,
            std::ptr::null(),
            hooks.as_ptr(),
            directory.as_ptr(),
            library_name.as_ptr(),
            redirect.as_ptr(),
            std::ptr::null_mut(),
        )
    };
    if vulkan.is_null() {
        return Err("AdrenoTools rejected the driver".into());
    }

    let vk_symbol = std::ffi::CString::new("vkGetInstanceProcAddr").unwrap();
    let proc_addr = unsafe { dlsym(vulkan, vk_symbol.as_ptr()) };
    if proc_addr.is_null() {
        Err("Driver does not export vkGetInstanceProcAddr".into())
    } else {
        Ok(proc_addr)
    }
}

#[cfg(all(target_os = "android", target_arch = "aarch64"))]
fn path_to_c_string(path: &Path) -> Result<std::ffi::CString, String> {
    std::ffi::CString::new(path.to_string_lossy().as_bytes()).map_err(|error| error.to_string())
}
