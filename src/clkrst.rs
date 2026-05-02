//! HP_SYS_CLKRST chip-wide clock setup for ESP32-P4.
//!
//! Programs the bits IDF's `bootloader_clock_configure()` and `pmu_init`
//! set in `HP_SYS_CLKRST` for HP-domain root/peripheral clocks. This is
//! distinct from the per-peripheral clock-source-select calls (PSRAM,
//! flash, etc) — those live with the consumer crate.
//!
//! Most critical: `SOC_CLK_CTRL1.PVT_SYS_CLK_EN` (bit 25). The PVT
//! (Process-Voltage-Temperature) monitor needs this clock to operate;
//! without it, analog blocks lose dynamic voltage adjustment and the
//! MSPI PHY can drive but cannot sample MISO data.
//!
//! Source: instrumented IDF v5.3 dump captured 2026-05-01.

#![allow(unsafe_code)]

use core::ptr;

const HP_SYS_CLKRST_BASE: usize = 0x500E_6000;

#[inline(always)]
unsafe fn cr_or(off: usize, set_bits: u32) {
    let p = (HP_SYS_CLKRST_BASE + off) as *mut u32;
    let cur = ptr::read_volatile(p);
    ptr::write_volatile(p, cur | set_bits);
}

/// Program `HP_SYS_CLKRST` to IDF baseline (HP root + PVT clock gates).
/// Idempotent — uses OR-in, never clears bits the chip already has.
///
/// # Safety
///
/// Single-hart at boot, before IRQs. Writes fixed MMIO HP_SYS_CLKRST
/// registers.
pub unsafe fn init_hp_clocks() {
    cr_or(0x04, 0x0000_0060); // ROOT_CLK_CTRL0 — CPU_CLK_DIV bits
    cr_or(0x18, 0x0200_0000); // SOC_CLK_CTRL1.PVT_SYS_CLK_EN (bit 25)
    cr_or(0x1C, 0x0000_0020); // SOC_CLK_CTRL2 (bit 5)
    cr_or(0xA0, 0x0000_0001); // PERI_CLK_CTRL23 (bit 0)
    cr_or(0xA4, 0x0101_0000); // PERI_CLK_CTRL24.PVT_CLK_DIV/PVT_CLK_EN
    cr_or(0xA8, 0x0000_0F01); // PERI_CLK_CTRL25.PVT_PERI_GROUP{1..4}_CLK_EN
    cr_or(0xBC, 0x0000_034C); // ANA_PLL_CTRL0
    cr_or(0xE0, 0x0000_0018); // CPU_CLK_STATUS0
}
