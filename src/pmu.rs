//! PMU (Power Management Unit) bring-up for ESP32-P4.
//!
//! Sets HP_ACTIVE / HP_SLEEP / HP_MODEM / LP_SLEEP power-state templates
//! and external LDO regulator tunings to IDF v5.3 baseline values.
//! Mirrors what `pmu_hp_system_init_default()` /
//! `pmu_lp_system_init_default()` / `pvt_auto_dbias_init()` do in
//! `esp_hw_support/port/esp32p4/pmu_init.c` + `pmu_pvt.c`.
//!
//! **Why this matters:** the MSPI PHY analog block (and other analog
//! peripherals — RF, Ethernet PHY, USB PHY) need the EXT_LDO regulators
//! at the IDF-tuned voltage to operate correctly. Chip POR defaults are
//! NOT correct — most notably `EXT_LDO_P1_0P1A_ANA @0x1D4` defaults to
//! `0xA0000000` but needs `0x57000000` for stable PSRAM MR-read.
//!
//! Without this, `psram::detect()` cmd/addr/dummy/MOSI all complete in
//! 2-4 SCK cycles but MISO sampling hangs `slv_st = 5` forever.
//!
//! Source: instrumented IDF v5.3 dump captured 2026-05-01.

#![allow(unsafe_code)]

use core::ptr;

const PMU_BASE: usize = 0x5011_5000;

#[inline(always)]
unsafe fn pmu_w(off: usize, v: u32) {
    ptr::write_volatile((PMU_BASE + off) as *mut u32, v);
}

/// Program PMU registers to IDF baseline state. Required before any
/// analog peripheral (PSRAM PHY, RF, Ethernet PHY) is exercised. Idempotent.
///
/// # Safety
///
/// Single-hart at boot, before IRQs. Writes fixed MMIO PMU registers.
pub unsafe fn init_active_state() {
    // HP_ACTIVE state — values for chip-running domain.
    pmu_w(0x14, 0x7F80_0000); // HP_ACTIVE_HP_CK_POWER
    pmu_w(0x18, 0x02EC_0000); // HP_ACTIVE_BIAS — analog bias
    pmu_w(0x1C, 0x0100_00A0); // HP_ACTIVE_BACKUP
    pmu_w(0x20, 0xFFFF_FFFF); // HP_ACTIVE_BACKUP_CLK
    pmu_w(0x24, 0x0800_0000); // HP_ACTIVE_SYSCLK
    pmu_w(0x28, 0xC060_37F0); // HP_ACTIVE_HP_REGULATOR0 (some bits R/O)

    // HP_SLEEP state templates — PMU consults these even in active mode.
    pmu_w(0x68, 0x0020_0000); // HP_SLEEP_DIG_POWER
    pmu_w(0x6C, 0x0000_0000); // HP_SLEEP_ICG_HP_FUNC
    pmu_w(0x70, 0x0000_0000); // HP_SLEEP_ICG_HP_APB
    pmu_w(0x78, 0x3100_0000); // HP_SLEEP_HP_SYS_CNTL
    pmu_w(0x7C, 0x00E0_0000); // HP_SLEEP_HP_CK_POWER
    pmu_w(0x80, 0xC080_0000); // HP_SLEEP_BIAS
    pmu_w(0x84, 0x1280_0200); // HP_SLEEP_BACKUP
    pmu_w(0x88, 0xFFFF_FFFF); // HP_SLEEP_BACKUP_CLK
    pmu_w(0x8C, 0x3000_0000); // HP_SLEEP_SYSCLK
    pmu_w(0x90, 0xC044_0000); // HP_SLEEP_HP_REGULATOR0
    pmu_w(0x98, 0x0000_0000); // HP_SLEEP_XTAL
    pmu_w(0x9C, 0xF040_0000); // HP_SLEEP_LP_REGULATOR0

    // LP_SLEEP state.
    pmu_w(0xB4, 0xC040_0000); // LP_SLEEP_LP_REGULATOR0
    pmu_w(0xBC, 0x0000_0000); // LP_SLEEP_XTAL

    // Clear chip POR defaults that IDF zeros, set IDF-only values.
    pmu_w(0xC4, 0x0000_0000);
    pmu_w(0xC8, 0xC000_0000);
    pmu_w(0xF4, 0x0000_0000);
    pmu_w(0xF8, 0x0000_0000);
    pmu_w(0xFC, 0x0000_0000);
    pmu_w(0x10C, 0x0000_0000);
    pmu_w(0x110, 0x0000_0000);
    pmu_w(0x12C, 0x0002_0000);
    pmu_w(0x164, 0x0002_0000);
    pmu_w(0x174, 0x0002_0000);

    // External LDO regulators — THE critical analog tuning. Without
    // these (especially +0x1D4 EXT_LDO_P1_0P1A_ANA = 0x57000000), MSPI
    // PHY can't sample MISO. Chip POR has 0xA0000000 which doesn't work.
    pmu_w(0x1B8, 0x4020_0100); // EXT_LDO_P0_0P1A
    pmu_w(0x1BC, 0xB100_0000); // EXT_LDO_P0_0P1A_ANA
    pmu_w(0x1C0, 0x4020_0000); // EXT_LDO_P0_0P2A
    pmu_w(0x1C4, 0xA000_0000); // EXT_LDO_P0_0P2A_ANA
    pmu_w(0x1C8, 0x4020_0000); // EXT_LDO_P0_0P3A
    pmu_w(0x1CC, 0xA000_0000); // EXT_LDO_P0_0P3A_ANA
    pmu_w(0x1D0, 0x4020_0180); // EXT_LDO_P1_0P1A
    pmu_w(0x1D4, 0x5700_0000); // EXT_LDO_P1_0P1A_ANA — THE one
    pmu_w(0x1D8, 0x4020_0000); // EXT_LDO_P1_0P2A
    pmu_w(0x1DC, 0xA000_0000); // EXT_LDO_P1_0P2A_ANA
    pmu_w(0x1E0, 0x4020_0000); // EXT_LDO_P1_0P3A
    pmu_w(0x1E4, 0xA000_0000); // EXT_LDO_P1_0P3A_ANA
}
