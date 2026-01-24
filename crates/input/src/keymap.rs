pub fn macos_to_windows_vk(macos_code: u32) -> u32 {
    match macos_code {
        0 => 0x41, // A
        1 => 0x53, // S
        2 => 0x44, // D
        3 => 0x46, // F
        4 => 0x48, // H
        5 => 0x47, // G
        6 => 0x5A, // Z
        7 => 0x58, // X
        8 => 0x43, // C
        9 => 0x56, // V
        11 => 0x42, // B
        12 => 0x51, // Q
        13 => 0x57, // W
        14 => 0x45, // E
        15 => 0x52, // R
        16 => 0x59, // Y
        17 => 0x54, // T
        18 => 0x31, // 1
        19 => 0x32, // 2
        20 => 0x33, // 3
        21 => 0x34, // 4
        22 => 0x36, // 6
        23 => 0x35, // 5
        24 => 0xBB, // =
        25 => 0x39, // 9
        26 => 0x37, // 7
        27 => 0xBD, // -
        28 => 0x38, // 8
        29 => 0x30, // 0
        30 => 0xDD, // ]
        31 => 0x4F, // O
        32 => 0x55, // U
        33 => 0xDB, // [
        34 => 0x49, // I
        35 => 0x50, // P
        36 => 0x0D, // Enter
        37 => 0x4C, // L
        38 => 0x4A, // J
        39 => 0xDE, // '
        40 => 0x4B, // K
        41 => 0xBA, // ;
        42 => 0xDC, // \
        43 => 0xBC, // ,
        44 => 0xBF, // /
        45 => 0x4E, // N
        46 => 0x4D, // M
        47 => 0xBE, // .
        48 => 0x09, // Tab
        49 => 0x20, // Space
        50 => 0xC0, // `
        51 => 0x08, // Backspace
        53 => 0x1B, // Escape
        
        // Modifiers (Approximate mappings)
        55 => 0x5B, // Command (mapped to Windows Key)
        56 => 0x10, // Shift
        57 => 0x14, // Caps Lock
        58 => 0x12, // Option (mapped to Alt)
        59 => 0x11, // Control
        
        // Arrow Keys
        123 => 0x25, // Left
        124 => 0x27, // Right
        125 => 0x28, // Down
        126 => 0x26, // Up
        
        // Default: return as is if no mapping found (risky but better than nothing for some)
        _ => macos_code,
    }
}
