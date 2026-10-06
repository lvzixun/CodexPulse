/// Native preferences are consulted at show/theme changes, not on every rendered frame.
pub fn transparency_allowed() -> bool {
    #[cfg(windows)]
    {
        use windows_sys::Win32::{
            System::Registry::{HKEY_CURRENT_USER, RRF_RT_REG_DWORD, RegGetValueW},
            UI::{
                Accessibility::{HCF_HIGHCONTRASTON, HIGHCONTRASTW},
                WindowsAndMessaging::{SPI_GETHIGHCONTRAST, SystemParametersInfoW},
            },
        };
        let mut contrast = HIGHCONTRASTW {
            cbSize: std::mem::size_of::<HIGHCONTRASTW>() as u32,
            dwFlags: 0,
            lpszDefaultScheme: std::ptr::null_mut(),
        };
        // All pointers refer to fixed-size stack values whose lengths match the Win32 API contract.
        unsafe {
            if SystemParametersInfoW(
                SPI_GETHIGHCONTRAST,
                contrast.cbSize,
                (&mut contrast as *mut HIGHCONTRASTW).cast(),
                0,
            ) == 0
            {
                return false;
            }
            if contrast.dwFlags & HCF_HIGHCONTRASTON != 0 {
                return false;
            }
            let key: Vec<u16> =
                "Software\\Microsoft\\Windows\\CurrentVersion\\Themes\\Personalize\0"
                    .encode_utf16()
                    .collect();
            let value: Vec<u16> = "EnableTransparency\0".encode_utf16().collect();
            let mut enabled = 1u32;
            let mut size = std::mem::size_of::<u32>() as u32;
            let result = RegGetValueW(
                HKEY_CURRENT_USER,
                key.as_ptr(),
                value.as_ptr(),
                RRF_RT_REG_DWORD,
                std::ptr::null_mut(),
                (&mut enabled as *mut u32).cast(),
                &mut size,
            );
            // An absent preference uses the OS default; other failures conservatively disable glass.
            result == 2 || (result == 0 && enabled != 0)
        }
    }
    #[cfg(not(windows))]
    {
        false
    }
}
