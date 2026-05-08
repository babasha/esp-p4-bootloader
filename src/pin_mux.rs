//! IOMUX_MSPI_PIN: drive-strength + DQS XPD setup for the PSRAM/Flash MSPI bank.
//!
//! PAC esp32p4 0.2 doesn't expose IOMUX_MSPI_PIN as a peripheral, so we use
//! raw read-modify-write at the documented register addresses. Layouts come
//! from `idf_ref_mspi_timing_tuning_ll.h` and IDF v5.4
//! `soc/iomux_mspi_pin_reg.h`.

#![allow(unsafe_code)]

use core::ptr;

/// `DR_REG_IOMUX_MSPI_PIN_BASE = HPPERIPH1 + 0x21200`.
const IOMUX_MSPI_PIN_BASE: usize = 0x500E_1200;

/// Per-pin offsets within `IOMUX_MSPI_PIN` for the 18 ordinary PSRAM
/// pins. Each register is 32-bit; bits [13:12] hold the `_DRV` field.
/// DQS_0 + DQS_1 are special-cased in [`IOMUX_PSRAM_DQS_OFFSETS`] (drv
/// shift = 15 instead of 12).
const IOMUX_PSRAM_PIN_OFFSETS: [u32; 18] = [
    0x1c, // PSRAM_D
    0x20, // PSRAM_Q
    0x24, // PSRAM_WP
    0x28, // PSRAM_HOLD
    0x2c, // PSRAM_DQ4
    0x30, // PSRAM_DQ5
    0x34, // PSRAM_DQ6
    0x38, // PSRAM_DQ7
    0x40, // PSRAM_CK
    0x44, // PSRAM_CS
    0x48, // PSRAM_DQ8
    0x4c, // PSRAM_DQ9
    0x50, // PSRAM_DQ10
    0x54, // PSRAM_DQ11
    0x58, // PSRAM_DQ12
    0x5c, // PSRAM_DQ13
    0x60, // PSRAM_DQ14
    0x64, // PSRAM_DQ15
];

/// DQS_0 / DQS_1 register offsets — DRV shift is 15 (not 12), and bit 0
/// is the `_XPD` enable.
const IOMUX_PSRAM_DQS_OFFSETS: [u32; 2] = [0x3c, 0x68];

const IOMUX_DRV_SHIFT_NORMAL: u32 = 12;
const IOMUX_DRV_SHIFT_DQS: u32 = 15;
const IOMUX_DRV_MASK: u32 = 0b11;
const IOMUX_DQS_XPD_BIT: u32 = 1 << 0;

#[inline(always)]
unsafe fn iomux_modify(offset: u32, mask: u32, value: u32) {
    let reg = (IOMUX_MSPI_PIN_BASE + offset as usize) as *mut u32;
    let old = ptr::read_volatile(reg);
    ptr::write_volatile(reg, (old & !mask) | (value & mask));
}

/// `mspi_timing_ll_pin_drv_set(drv)` — writes the 2-bit DRV field on all 21
/// PSRAM pin registers (D/Q/WP/HOLD, DQ4..DQ7, DQ8..DQ15, DQS_0/DQS_1, CK,
/// CS). Source: `mspi_timing_tuning_ll.h:179`.
#[inline]
pub fn pin_drv_set(drv: u8) {
    let drv = (drv as u32) & IOMUX_DRV_MASK;
    let normal_mask = IOMUX_DRV_MASK << IOMUX_DRV_SHIFT_NORMAL;
    let normal_val = drv << IOMUX_DRV_SHIFT_NORMAL;
    let dqs_mask = IOMUX_DRV_MASK << IOMUX_DRV_SHIFT_DQS;
    let dqs_val = drv << IOMUX_DRV_SHIFT_DQS;

    // SAFETY: each address is a fixed MMIO register; read-modify-write is
    // single-hart at boot before IRQs.
    unsafe {
        for &off in &IOMUX_PSRAM_PIN_OFFSETS {
            iomux_modify(off, normal_mask, normal_val);
        }
        for &off in &IOMUX_PSRAM_DQS_OFFSETS {
            iomux_modify(off, dqs_mask, dqs_val);
        }
    }
}

/// `mspi_timing_ll_enable_dqs(en)` — toggles `_XPD` (output enable) on
/// DQS_0 and DQS_1 pin regs. Source: `mspi_timing_tuning_ll.h:161`.
#[inline]
pub fn enable_dqs(en: bool) {
    // SAFETY: same justification as `pin_drv_set`.
    unsafe {
        for &off in &IOMUX_PSRAM_DQS_OFFSETS {
            let v = if en { IOMUX_DQS_XPD_BIT } else { 0 };
            iomux_modify(off, IOMUX_DQS_XPD_BIT, v);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// DRV mask/shift math constants — sanity-check encoding for drv=2
    /// (the value `init()` passes). For ordinary pins shift 12: mask
    /// 0x3000, value 0x2000; for DQS shift 15: mask 0x18000, value 0x10000.
    #[test]
    fn drv_encoding_drv2() {
        let drv = 2u32 & IOMUX_DRV_MASK;
        assert_eq!(drv << IOMUX_DRV_SHIFT_NORMAL, 0x0000_2000);
        assert_eq!(IOMUX_DRV_MASK << IOMUX_DRV_SHIFT_NORMAL, 0x0000_3000);
        assert_eq!(drv << IOMUX_DRV_SHIFT_DQS, 0x0001_0000);
        assert_eq!(IOMUX_DRV_MASK << IOMUX_DRV_SHIFT_DQS, 0x0001_8000);
    }

    /// 18 ordinary PSRAM pins + 2 DQS = 20 pins covered.
    #[test]
    fn pin_table_size() {
        assert_eq!(IOMUX_PSRAM_PIN_OFFSETS.len(), 18);
        assert_eq!(IOMUX_PSRAM_DQS_OFFSETS.len(), 2);
    }
}
