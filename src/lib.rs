//! ESP32-P4 hardware bring-up — pure-Rust equivalent of IDF's
//! `bootloader_init()` for the `--ram --no-stub` boot flow.
//!
//! This crate brings the chip from ROM-bootloader-handoff state into a
//! fully-configured state ready for any analog peripheral (PSRAM, EMAC,
//! USB) to be initialised. Specifically:
//!
//! - **BOD/WDT off** so the chip doesn't reset itself during long init.
//! - **PMU regulators + power-state templates** to IDF baseline. The
//!   `EXT_LDO_*` block in particular is required for MSPI PHY analog
//!   sampling (without correct LDO voltage, controllers can drive but
//!   not sample data).
//! - **MPLL @ 400 MHz** as the PSRAM/MSPI clock source.
//! - **HP_SYS_CLKRST chip-wide clocks** including the PVT monitor.
//! - **MSPI pin DRV/DQS** for the dedicated PSRAM/flash pads.
//! - **Flash MSPI** init via ROM `spi_flash_attach` + CS timing + resume +
//!   unlock + WP.
//! - **L2 cache mode** + `Cache_Enable_L2_Cache` + `mmu_hal_init`.
//!
//! After [`init_phase2_full`] returns, downstream crates can do their
//! own peripheral init (e.g. `psram::init()`) without worrying about
//! analog/clock state.

#![cfg_attr(not(test), no_std)]

#[cfg(any(target_arch = "riscv32", test))]
pub mod regi2c;
#[cfg(any(target_arch = "riscv32", test))]
pub mod pin_mux;
#[cfg(any(target_arch = "riscv32", test))]
pub mod mmu;

#[cfg(target_arch = "riscv32")]
pub mod bod;
#[cfg(target_arch = "riscv32")]
pub mod mpll;
#[cfg(target_arch = "riscv32")]
pub mod cache;
#[cfg(target_arch = "riscv32")]
pub mod flash;
#[cfg(target_arch = "riscv32")]
pub mod pmu;
#[cfg(target_arch = "riscv32")]
pub mod clkrst;
pub mod wdt;
#[cfg(target_arch = "riscv32")]
pub mod reset_cause;

/// Phase-1 hardware bring-up — minimum to keep the chip alive: BOD off,
/// WDT off, L2 cache mode set. Runs in a few microseconds.
///
/// Use this when you only need to disable resets and bring up the L2
/// cache mode shadow registers (e.g. for the EMAC driver's cold-boot
/// fix in `feedback_p4_cache_rom_hang.md`). For analog peripherals
/// (PSRAM, RF) call [`init_phase2_full`] instead.
#[cfg(target_arch = "riscv32")]
pub fn init() {
    bod::disable();
    wdt::disable_all();
    cache::init_l2_cache_mode();
}

/// Phase-2 hardware bring-up — full chip-wide init ready for analog
/// peripherals. Order:
///
/// 1. `bod::disable` + `wdt::disable_all` — stop self-resets.
/// 2. `pmu::init_active_state` — HP_ACTIVE/SLEEP/MODEM templates +
///    EXT_LDO regulator tuning. **Required** before MSPI PHY operation.
/// 3. `mpll::bringup_400` — MPLL up to 400 MHz from XTAL via REGI2C.
/// 4. `clkrst::init_hp_clocks` — HP_SYS_CLKRST PVT clock gates and
///    HP-root clock dividers.
/// 5. `pin_mux::pin_drv_set(2)` + `enable_dqs(true)` — IOMUX_MSPI_PIN
///    drive strength + DQS XPD.
/// 6. `flash::set_spll_clock_rev1plus` — switch flash MSPI to SPLL clock
///    (rev1+ ESP32-P4 only).
/// 7. `cache::init_l2_cache_mode` — `Cache_Set_L2_Cache_Mode(256K, 8w, 64B)`
///    + `Cache_Invalidate_All(L2)` ROM calls.
/// 8. `cache::hal_init` — `Cache_Enable_L2_Cache` ROM call.
/// 9. `mmu::hal_init` — invalidate all flash + PSRAM MMU entries.
/// 10. `flash::bootloader_init_spi_flash` — `spi_flash_attach` + CS
///     timing + resume + unlock + WP.
///
/// **Pre-condition:** chip is at the post-ROM-bootloader state from
/// `espflash --ram --no-stub`. Call once from `_start` / `#[entry] fn main`.
///
/// **Post-condition:** chip is ready for `psram::init()` (or any other
/// analog peripheral driver) to take over.
#[cfg(target_arch = "riscv32")]
pub fn init_phase2_full() {
    bod::disable();
    wdt::disable_all();
    // SAFETY: single-hart at boot, before IRQs.
    unsafe {
        pmu::init_active_state();
        mpll::bringup_400();
        clkrst::init_hp_clocks();
    }
    pin_mux::pin_drv_set(2);
    pin_mux::enable_dqs(true);
    // SAFETY: single-hart at boot.
    unsafe {
        flash::set_spll_clock_rev1plus();
    }
    cache::init_l2_cache_mode();
    cache::hal_init();
    mmu::hal_init();
    flash::bootloader_init_spi_flash();
}

/// Host build no-op so workspace tests link.
#[cfg(not(target_arch = "riscv32"))]
pub fn init() {}
