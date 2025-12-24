// Copyright (c) Microsoft Corporation.
// Licensed under the MIT License.

//! Host SMBIOS data extraction for VM stealth mode.
//!
//! This module provides cross-platform functionality to query the host system's
//! SMBIOS data (manufacturer, serial numbers, etc.) for mirroring into the VM
//! firmware. This makes the VM appear as the host hardware in SMBIOS queries.

use anyhow::Context;
use anyhow::Result;

/// Host SMBIOS data that can be mirrored into a VM.
#[derive(Debug, Clone, Default)]
pub struct HostSmbiosData {
    /// System serial number (Type 1, offset 0x07)
    pub system_serial_number: String,
    /// System manufacturer (Type 1, offset 0x04)
    pub system_manufacturer: String,
    /// System product name (Type 1, offset 0x05)
    pub system_product_name: String,
    /// System version (Type 1, offset 0x06)
    pub system_version: String,
    /// System SKU number (Type 1, offset 0x19)
    pub system_sku_number: String,
    /// System family (Type 1, offset 0x1A)
    pub system_family: String,
    /// System UUID (Type 1, offset 0x08)
    pub system_uuid: Option<[u8; 16]>,

    /// Base board (motherboard) serial number (Type 2, offset 0x07)
    pub baseboard_serial_number: String,
    /// Base board manufacturer (Type 2, offset 0x04)
    pub baseboard_manufacturer: String,
    /// Base board product name (Type 2, offset 0x05)
    pub baseboard_product: String,

    /// Chassis serial number (Type 3, offset 0x07)
    pub chassis_serial_number: String,
    /// Chassis asset tag (Type 3, offset 0x08)
    pub chassis_asset_tag: String,
    /// Chassis manufacturer (Type 3, offset 0x04)
    pub chassis_manufacturer: String,

    /// BIOS vendor (Type 0, offset 0x04)
    pub bios_vendor: String,
    /// BIOS version (Type 0, offset 0x05)
    pub bios_version: String,

    /// Processor manufacturer (Type 4, offset 0x07)
    pub processor_manufacturer: String,
    /// Processor version (Type 4, offset 0x10)
    pub processor_version: String,
}

impl HostSmbiosData {
    /// Query the host system's SMBIOS data.
    ///
    /// On Windows, this uses WMI queries.
    /// On Linux, this reads from /sys/class/dmi/id/ or falls back to dmidecode.
    pub fn query() -> Result<Self> {
        #[cfg(windows)]
        {
            query_windows()
        }
        #[cfg(target_os = "linux")]
        {
            query_linux()
        }
        #[cfg(not(any(windows, target_os = "linux")))]
        {
            anyhow::bail!("Host SMBIOS query not supported on this platform")
        }
    }
}

/// Windows implementation using WMI via PowerShell.
///
/// We use PowerShell to query WMI classes because:
/// 1. It avoids COM initialization complexity in Rust
/// 2. It's reliable and well-tested
/// 3. Performance is acceptable (one-time query at VM startup)
#[cfg(windows)]
fn query_windows() -> Result<HostSmbiosData> {
    use std::process::Command;

    let mut data = HostSmbiosData::default();

    // Query Win32_ComputerSystem for system info
    let output = Command::new("powershell")
        .args([
            "-NoProfile",
            "-Command",
            r#"Get-WmiObject Win32_ComputerSystem | Select-Object Manufacturer, Model, SystemFamily, SystemSKUNumber | ConvertTo-Json"#,
        ])
        .output()
        .context("Failed to execute PowerShell for Win32_ComputerSystem")?;

    if output.status.success() {
        if let Ok(json) = String::from_utf8(output.stdout) {
            parse_computer_system_json(&json, &mut data);
        }
    }

    // Query Win32_ComputerSystemProduct for UUID and serial
    let output = Command::new("powershell")
        .args([
            "-NoProfile",
            "-Command",
            r#"Get-WmiObject Win32_ComputerSystemProduct | Select-Object UUID, Vendor, Version, IdentifyingNumber | ConvertTo-Json"#,
        ])
        .output()
        .context("Failed to execute PowerShell for Win32_ComputerSystemProduct")?;

    if output.status.success() {
        if let Ok(json) = String::from_utf8(output.stdout) {
            parse_computer_system_product_json(&json, &mut data);
        }
    }

    // Query Win32_BaseBoard for motherboard info
    let output = Command::new("powershell")
        .args([
            "-NoProfile",
            "-Command",
            r#"Get-WmiObject Win32_BaseBoard | Select-Object Manufacturer, Product, SerialNumber | ConvertTo-Json"#,
        ])
        .output()
        .context("Failed to execute PowerShell for Win32_BaseBoard")?;

    if output.status.success() {
        if let Ok(json) = String::from_utf8(output.stdout) {
            parse_baseboard_json(&json, &mut data);
        }
    }

    // Query Win32_SystemEnclosure for chassis info
    let output = Command::new("powershell")
        .args([
            "-NoProfile",
            "-Command",
            r#"Get-WmiObject Win32_SystemEnclosure | Select-Object Manufacturer, SerialNumber, SMBIOSAssetTag | ConvertTo-Json"#,
        ])
        .output()
        .context("Failed to execute PowerShell for Win32_SystemEnclosure")?;

    if output.status.success() {
        if let Ok(json) = String::from_utf8(output.stdout) {
            parse_system_enclosure_json(&json, &mut data);
        }
    }

    // Query Win32_BIOS for BIOS info
    let output = Command::new("powershell")
        .args([
            "-NoProfile",
            "-Command",
            r#"Get-WmiObject Win32_BIOS | Select-Object Manufacturer, SMBIOSBIOSVersion, SerialNumber | ConvertTo-Json"#,
        ])
        .output()
        .context("Failed to execute PowerShell for Win32_BIOS")?;

    if output.status.success() {
        if let Ok(json) = String::from_utf8(output.stdout) {
            parse_bios_json(&json, &mut data);
        }
    }

    // Query Win32_Processor for CPU info
    let output = Command::new("powershell")
        .args([
            "-NoProfile",
            "-Command",
            r#"Get-WmiObject Win32_Processor | Select-Object -First 1 Manufacturer, Name | ConvertTo-Json"#,
        ])
        .output()
        .context("Failed to execute PowerShell for Win32_Processor")?;

    if output.status.success() {
        if let Ok(json) = String::from_utf8(output.stdout) {
            parse_processor_json(&json, &mut data);
        }
    }

    Ok(data)
}

/// Parse Win32_ComputerSystem JSON output
#[cfg(windows)]
fn parse_computer_system_json(json: &str, data: &mut HostSmbiosData) {
    // Simple JSON parsing without external dependencies
    // Format: {"Manufacturer":"...", "Model":"...", ...}
    if let Some(manufacturer) = extract_json_string(json, "Manufacturer") {
        data.system_manufacturer = manufacturer;
    }
    if let Some(model) = extract_json_string(json, "Model") {
        data.system_product_name = model;
    }
    if let Some(family) = extract_json_string(json, "SystemFamily") {
        data.system_family = family;
    }
    if let Some(sku) = extract_json_string(json, "SystemSKUNumber") {
        data.system_sku_number = sku;
    }
}

/// Parse Win32_ComputerSystemProduct JSON output
#[cfg(windows)]
fn parse_computer_system_product_json(json: &str, data: &mut HostSmbiosData) {
    if let Some(serial) = extract_json_string(json, "IdentifyingNumber") {
        data.system_serial_number = serial;
    }
    if let Some(version) = extract_json_string(json, "Version") {
        data.system_version = version;
    }
    if let Some(uuid_str) = extract_json_string(json, "UUID") {
        data.system_uuid = parse_uuid_string(&uuid_str);
    }
}

/// Parse Win32_BaseBoard JSON output
#[cfg(windows)]
fn parse_baseboard_json(json: &str, data: &mut HostSmbiosData) {
    if let Some(manufacturer) = extract_json_string(json, "Manufacturer") {
        data.baseboard_manufacturer = manufacturer;
    }
    if let Some(product) = extract_json_string(json, "Product") {
        data.baseboard_product = product;
    }
    if let Some(serial) = extract_json_string(json, "SerialNumber") {
        data.baseboard_serial_number = serial;
    }
}

/// Parse Win32_SystemEnclosure JSON output
#[cfg(windows)]
fn parse_system_enclosure_json(json: &str, data: &mut HostSmbiosData) {
    if let Some(manufacturer) = extract_json_string(json, "Manufacturer") {
        data.chassis_manufacturer = manufacturer;
    }
    if let Some(serial) = extract_json_string(json, "SerialNumber") {
        data.chassis_serial_number = serial;
    }
    if let Some(asset_tag) = extract_json_string(json, "SMBIOSAssetTag") {
        data.chassis_asset_tag = asset_tag;
    }
}

/// Parse Win32_BIOS JSON output
#[cfg(windows)]
fn parse_bios_json(json: &str, data: &mut HostSmbiosData) {
    if let Some(vendor) = extract_json_string(json, "Manufacturer") {
        data.bios_vendor = vendor;
    }
    if let Some(version) = extract_json_string(json, "SMBIOSBIOSVersion") {
        data.bios_version = version;
    }
}

/// Parse Win32_Processor JSON output
#[cfg(windows)]
fn parse_processor_json(json: &str, data: &mut HostSmbiosData) {
    if let Some(manufacturer) = extract_json_string(json, "Manufacturer") {
        data.processor_manufacturer = manufacturer;
    }
    if let Some(name) = extract_json_string(json, "Name") {
        data.processor_version = name;
    }
}

/// Extract a string value from simple JSON.
/// Handles both {"key": "value"} and {"key": null} patterns.
#[cfg(windows)]
fn extract_json_string(json: &str, key: &str) -> Option<String> {
    // Look for "key": "value" or "key":"value"
    let pattern = format!("\"{}\"", key);
    let key_pos = json.find(&pattern)?;
    let after_key = &json[key_pos + pattern.len()..];

    // Skip whitespace and colon
    let after_colon = after_key.trim_start();
    let after_colon = after_colon.strip_prefix(':')?;
    let after_colon = after_colon.trim_start();

    // Check for null
    if after_colon.starts_with("null") {
        return None;
    }

    // Check for quoted string
    if !after_colon.starts_with('"') {
        return None;
    }

    let value_start = 1; // Skip opening quote
    let value_content = &after_colon[value_start..];

    // Find closing quote (handle escaped quotes)
    let mut end_pos = 0;
    let mut chars = value_content.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\\' {
            chars.next(); // Skip escaped char
            end_pos += 2;
        } else if c == '"' {
            break;
        } else {
            end_pos += c.len_utf8();
        }
    }

    let value = &value_content[..end_pos];

    // Unescape the string
    let unescaped = value
        .replace("\\\"", "\"")
        .replace("\\\\", "\\")
        .replace("\\n", "\n")
        .replace("\\r", "\r")
        .replace("\\t", "\t");

    if unescaped.is_empty() {
        None
    } else {
        Some(unescaped)
    }
}

/// Parse a UUID string in the format "XXXXXXXX-XXXX-XXXX-XXXX-XXXXXXXXXXXX"
fn parse_uuid_string(uuid_str: &str) -> Option<[u8; 16]> {
    let hex_str: String = uuid_str.chars().filter(|c| c.is_ascii_hexdigit()).collect();
    if hex_str.len() != 32 {
        return None;
    }

    let mut bytes = [0u8; 16];
    for i in 0..16 {
        bytes[i] = u8::from_str_radix(&hex_str[i*2..i*2+2], 16).ok()?;
    }
    Some(bytes)
}

/// Linux implementation reading from /sys/class/dmi/id/
#[cfg(target_os = "linux")]
fn query_linux() -> Result<HostSmbiosData> {
    use std::fs;
    use std::path::Path;

    let dmi_path = Path::new("/sys/class/dmi/id");

    // Helper to read a DMI file and trim whitespace
    let read_dmi = |filename: &str| -> String {
        fs::read_to_string(dmi_path.join(filename))
            .map(|s| s.trim().to_string())
            .unwrap_or_default()
    };

    let data = HostSmbiosData {
        // System (Type 1)
        system_serial_number: read_dmi("product_serial"),
        system_manufacturer: read_dmi("sys_vendor"),
        system_product_name: read_dmi("product_name"),
        system_version: read_dmi("product_version"),
        system_sku_number: read_dmi("product_sku"),
        system_family: read_dmi("product_family"),
        system_uuid: read_product_uuid(),

        // Base board (Type 2)
        baseboard_serial_number: read_dmi("board_serial"),
        baseboard_manufacturer: read_dmi("board_vendor"),
        baseboard_product: read_dmi("board_name"),

        // Chassis (Type 3)
        chassis_serial_number: read_dmi("chassis_serial"),
        chassis_asset_tag: read_dmi("chassis_asset_tag"),
        chassis_manufacturer: read_dmi("chassis_vendor"),

        // BIOS (Type 0)
        bios_vendor: read_dmi("bios_vendor"),
        bios_version: read_dmi("bios_version"),

        // Processor (Type 4) - not available via /sys/class/dmi/id
        // Would need to parse /proc/cpuinfo or use dmidecode
        processor_manufacturer: String::new(),
        processor_version: read_processor_info(),
    };

    Ok(data)
}

/// Read the product UUID from /sys/class/dmi/id/product_uuid
#[cfg(target_os = "linux")]
fn read_product_uuid() -> Option<[u8; 16]> {
    use std::fs;
    let uuid_str = fs::read_to_string("/sys/class/dmi/id/product_uuid").ok()?;
    parse_uuid_string(uuid_str.trim())
}

/// Read processor info from /proc/cpuinfo
#[cfg(target_os = "linux")]
fn read_processor_info() -> String {
    use std::fs;

    if let Ok(cpuinfo) = fs::read_to_string("/proc/cpuinfo") {
        for line in cpuinfo.lines() {
            if line.starts_with("model name") {
                if let Some((_, value)) = line.split_once(':') {
                    return value.trim().to_string();
                }
            }
        }
    }
    String::new()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_uuid_string() {
        let uuid = parse_uuid_string("12345678-1234-5678-1234-567812345678");
        assert!(uuid.is_some());
        let bytes = uuid.unwrap();
        assert_eq!(bytes[0], 0x12);
        assert_eq!(bytes[1], 0x34);
    }

    #[test]
    fn test_parse_uuid_string_invalid() {
        assert!(parse_uuid_string("not-a-uuid").is_none());
        assert!(parse_uuid_string("").is_none());
    }

    #[cfg(windows)]
    #[test]
    fn test_extract_json_string() {
        let json = r#"{"Manufacturer": "Framework", "Model": "Laptop 16"}"#;
        assert_eq!(extract_json_string(json, "Manufacturer"), Some("Framework".to_string()));
        assert_eq!(extract_json_string(json, "Model"), Some("Laptop 16".to_string()));
        assert_eq!(extract_json_string(json, "NotPresent"), None);
    }

    #[cfg(windows)]
    #[test]
    fn test_extract_json_string_null() {
        let json = r#"{"Field": null}"#;
        assert_eq!(extract_json_string(json, "Field"), None);
    }

    #[cfg(windows)]
    #[test]
    fn test_extract_json_string_escaped() {
        let json = r#"{"Name": "Test \"Quoted\" Value"}"#;
        assert_eq!(extract_json_string(json, "Name"), Some("Test \"Quoted\" Value".to_string()));
    }
}
