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
        
        // Modifiers
        55 => 0x5B, // Command -> Windows Key
        56 => 0x10, // Shift
        57 => 0x14, // Caps Lock
        58 => 0x12, // Option -> Alt
        59 => 0x11, // Control
        
        // Arrows
        123 => 0x25, // Left
        124 => 0x27, // Right
        125 => 0x28, // Down
        126 => 0x26, // Up
        
        _ => macos_code,
    }
}

pub fn windows_to_macos_keycode(win_vk: u32) -> u16 {
    match win_vk {
        0x41 => 0,   // A
        0x53 => 1,   // S
        0x44 => 2,   // D
        0x46 => 3,   // F
        0x48 => 4,   // H
        0x47 => 5,   // G
        0x5A => 6,   // Z
        0x58 => 7,   // X
        0x43 => 8,   // C
        0x56 => 9,   // V
        0x42 => 11,  // B
        0x51 => 12,  // Q
        0x57 => 13,  // W
        0x45 => 14,  // E
        0x52 => 15,  // R
        0x59 => 16,  // Y
        0x54 => 17,  // T
        0x31 => 18,  // 1
        0x32 => 19,  // 2
        0x33 => 20,  // 3
        0x34 => 21,  // 4
        0x36 => 22,  // 6
        0x35 => 23,  // 5
        0xBB => 24,  // =
        0x39 => 25,  // 9
        0x37 => 26,  // 7
        0xBD => 27,  // -
        0x38 => 28,  // 8
        0x30 => 29,  // 0
        0xDD => 30,  // ]
        0x4F => 31,  // O
        0x55 => 32,  // U
        0xDB => 33,  // [
        0x49 => 34,  // I
        0x50 => 35,  // P
        0x0D => 36,  // Enter
        0x4C => 37,  // L
        0x4A => 38,  // J
        0xDE => 39,  // '
        0x4B => 40,  // K
        0xBA => 41,  // ;
        0xDC => 42,  // \
        0xBC => 43,  // ,
        0xBF => 44,  // /
        0x4E => 45,  // N
        0x4D => 46,  // M
        0xBE => 47,  // .
        0x09 => 48,  // Tab
        0x20 => 49,  // Space
        0xC0 => 50,  // `
        0x08 => 51,  // Backspace
        0x1B => 53,  // Escape
        
        // Modifiers
        0x5B => 55,  // Windows Key -> Command
        0x10 => 56,  // Shift
        0x14 => 57,  // Caps Lock
        0x12 => 58,  // Alt -> Option
        0x11 => 59,  // Control
        
        // Arrows
        0x25 => 123, // Left
        0x27 => 124, // Right
        0x28 => 125, // Down
        0x26 => 126, // Up

        _ => win_vk as u16,
    }
}
