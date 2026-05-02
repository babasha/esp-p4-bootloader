//! Flash MSPI bring-up. Mirrors IDF v5.3
//! `bootloader_flash_hardware_init()` from
//! `idf_v53_ref/bootloader_flash_config_p4.c:265` (the
//! `CONFIG_APP_BUILD_TYPE_RAM` path — our `--ram --no-stub` flow is the
//! same execution model).
//!
//! The pieces we **already** do elsewhere and skip here:
//! - `cache_hal_init()` — handled by [`crate::cache::hal_init`] earlier in
//!   `init()`.
//! - `mmu_hal_init()` — handled by [`crate::mmu::hal_init`].
//! - `bootloader_configure_spi_pins(1)` — on P4 every `SPI_*_GPIO_NUM` is
//!   set to `GPIO_NUM_MAX`, so the IDF call is a no-op anyway. Pin DRV
//!   on the actual MSPI pins is set in [`crate::pin_mux::pin_drv_set`].
//! - `bootloader_flash_set_spi_mode` / `bootloader_flash_clock_config` —
//!   ROM bootloader has already configured DIO + the system clock by the
//!   time it hands off to us; rewriting them risks conflict.
//! - XMC startup, `update_id`, `update_flash_config` — these are flash
//!   chip-side metadata; we never read flash in `--ram --no-stub`.
//!
//! What we **do** here, in order:
//! 1. `esp_rom_spiflash_attach(0, false)` — main MSPI controller setup.
//!    Most likely the missing piece for PSRAM signaling — flash and PSRAM
//!    banks share MSPI PHY/timing/sampling state. Source:
//!    `esp32p4.rom.ld:162` (symbol `spi_flash_attach`).
//! 2. `flash_cs_timing_config` — set CS_HOLD/CS_SETUP on SPIMEM0 (PAC
//!    `SPI0`, base `0x5008_C000`) with `*_TIME = 0`.
//! 3. `spi_flash_resume` — send 0xAB (Release Deep Power Down) on SPIMEM1
//!    (PAC `SPI1`, base `0x5008_D000`).
//! 4. `esp_rom_spiflash_unlock()` — ROM helper that drives WREN/WRSR for
//!    whichever flash vendor IDF detects. `0x4FC0_015C`.
//! 5. `enable_wp` — send WRDI to put WP back on.

#![allow(unsafe_code)]

// ── ROM symbol addresses (esp32p4.rom.ld v5.3) ──────────────────────────────

/// `void spi_flash_attach(uint32_t ishspi, bool legacy);`
const ROM_SPI_FLASH_ATTACH: usize = 0x4FC0_01E8;

/// `esp_rom_spiflash_result_t esp_rom_spiflash_unlock(void);`
const ROM_ESP_ROM_SPIFLASH_UNLOCK: usize = 0x4FC0_015C;

type RomSpiFlashAttach = unsafe extern "C" fn(ishspi: u32, legacy: bool);
type RomSpiflashUnlock = unsafe extern "C" fn() -> i32;

// ── Standard SPI NOR flash command opcodes ──────────────────────────────────

const CMD_RESUME: u8 = 0xAB; // Release Deep Power Down
const CMD_WRDI: u8 = 0x04; // Write Disable

// ── Peripheral handles ──────────────────────────────────────────────────────

/// PAC handle for `HP_SYS_CLKRST`.
#[inline(always)]
fn clkrst() -> &'static esp32p4::hp_sys_clkrst::RegisterBlock {
    // SAFETY: HP_SYS_CLKRST::PTR is the PAC-provided MMIO base.
    unsafe { &*esp32p4::HP_SYS_CLKRST::PTR }
}

/// PAC handle for SPIMEM0 (FLASH_SPI0, base `0x5008_C000`). IDF name:
/// `SPIMEM0`. Used for the AXI/cache-side flash controller (CS timing).
#[inline(always)]
fn spi0() -> &'static esp32p4::spi0::RegisterBlock {
    // SAFETY: SPI0::PTR is the PAC-provided MMIO base.
    unsafe { &*esp32p4::SPI0::PTR }
}

/// PAC handle for SPIMEM1 (FLASH_SPI1, base `0x5008_D000`). IDF name:
/// `SPIMEM1` / `SPIFLASH`. Used for user-mode flash transactions.
#[inline(always)]
fn spi1() -> &'static esp32p4::spi1::RegisterBlock {
    // SAFETY: SPI1::PTR is the PAC-provided MMIO base.
    unsafe { &*esp32p4::SPI1::PTR }
}

// ── Public API ──────────────────────────────────────────────────────────────

/// IDF `bootloader_hardware_init`'s rev-1+ branch:
/// `spimem_flash_ll_select_clk_source(0, FLASH_CLK_SRC_SPLL)` +
/// `spimem_ctrlr_ll_set_core_clock(0, 6)`. Sets flash MSPI clock to SPLL
/// with core divider 6. ROM bootloader leaves flash on XTAL; 2nd-stage
/// bootloader switches it to SPLL. Because flash and PSRAM share MSPI
/// infrastructure, this likely settles shared PHY state needed for PSRAM.
///
/// # Safety
///
/// Touches HP_SYS_CLKRST. Single-hart at boot.
pub unsafe fn set_spll_clock_rev1plus() {
    let r = clkrst();
    r.peri_clk_ctrl00().modify(|_, w| {
        w.flash_pll_clk_en().set_bit();
        // SAFETY: 2-bit field, value 1 (SPLL) is valid.
        unsafe { w.flash_clk_src_sel().bits(1) }
    });
    r.peri_clk_ctrl00().modify(|_, w| {
        w.flash_core_clk_en().set_bit();
        // SAFETY: 8-bit field, value 5 (= div 6) is valid.
        unsafe { w.flash_core_clk_div_num().bits(5) }
    });
}

/// IDF `bootloader_init_spi_flash` for the RAM-app boot path. See module
/// docs for what this skips and why.
pub fn bootloader_init_spi_flash() {
    // SAFETY: single-hart at boot, before IRQs. Each sub-step is documented
    // in module docs. `spi_flash_attach` is re-callable (IDF does the same
    // in `bootloader_flash_hardware_init`).
    unsafe {
        let attach: RomSpiFlashAttach = core::mem::transmute(ROM_SPI_FLASH_ATTACH);
        attach(0, false);

        flash_cs_timing_config();
        spi_flash_resume();

        let unlock: RomSpiflashUnlock = core::mem::transmute(ROM_ESP_ROM_SPIFLASH_UNLOCK);
        let _ = unlock();

        enable_wp();
    }
}

// ── CS timing on SPIMEM0 ────────────────────────────────────────────────────

/// Mirror of IDF `bootloader_flash_cs_timing_config()`. Sets the
/// CS_HOLD/CS_SETUP enable bits in `SPI_MEM_C_USER_REG` and zeroes the
/// `*_TIME` fields in `SPI_MEM_C_CTRL2_REG` — i.e., enable CS framing
/// with zero-cycle setup/hold. Source:
/// `bootloader_flash_config_p4.c:36-41`.
#[inline]
unsafe fn flash_cs_timing_config() {
    let s = spi0();
    s.user().modify(|_, w| {
        w.cs_hold().set_bit();
        w.cs_setup().set_bit()
    });
    s.ctrl2().modify(|_, w| {
        // SAFETY: 5-bit fields; value 0 fits trivially.
        unsafe {
            w.cs_hold_time().bits(0);
            w.cs_setup_time().bits(0)
        }
    });
}

// ── SPI flash command execution on SPIMEM1 ──────────────────────────────────

/// Mirror of IDF `bootloader_flash_execute_command_common()`. Sends a
/// short flash-command transaction via SPIMEM1: command byte (always 7
/// bits + 1 cmd-bit ⇒ 8-bit cmd), optional MOSI/MISO data, no addr/dummy.
/// Saves and restores USER/USER1/USER2/CTRL. Source:
/// `bootloader_flash.c:581-650` (P4 path: `usr_mosi_bit_len`,
/// `usr_miso_bit_len`).
///
/// # Safety
///
/// `mosi_len` ≤ 32, `miso_len` ≤ 32. Single-hart at boot.
unsafe fn execute_flash_command(command: u8, mosi_data: u32, mosi_len: u8, miso_len: u8) -> u32 {
    debug_assert!(mosi_len <= 32);
    debug_assert!(miso_len <= 32);

    let s = spi1();
    let old_ctrl = s.ctrl().read().bits();
    let old_user = s.user().read().bits();
    let old_user1 = s.user1().read().bits();
    let old_user2 = s.user2().read().bits();

    // Reset CTRL → 0, then assert WP=1 (matches IDF spimem_flash_ll_set_wp_level).
    s.ctrl().write(|w| unsafe { w.bits(0) });
    s.ctrl().modify(|_, w| w.wp().set_bit());

    // user phase: enable command, mosi/miso based on lengths; clear addr/dummy.
    s.user().modify(|_, w| {
        w.usr_command().set_bit();
        w.usr_addr().clear_bit();
        w.usr_dummy().clear_bit();
        w.usr_mosi().bit(mosi_len > 0);
        w.usr_miso().bit(miso_len > 0)
    });

    s.user2().modify(|_, w| {
        // SAFETY: usr_command_bitlen is 4-bit; 7 fits.
        unsafe { w.usr_command_bitlen().bits(7) };
        // SAFETY: usr_command_value is 16-bit; we pass an 8-bit cmd.
        unsafe { w.usr_command_value().bits(command as u16) }
    });

    // Zero addr/dummy/addr-bitlen — single 8-bit cmd, no addr or dummy.
    s.user1().write(|w| unsafe { w.bits(0) });
    s.addr().write(|w| unsafe { w.bits(0) });

    if mosi_len > 0 {
        // SAFETY: 10-bit field, max value 31 fits.
        s.mosi_dlen().write(|w| unsafe { w.usr_mosi_dbitlen().bits((mosi_len - 1) as u16) });
        s.w0().write(|w| unsafe { w.bits(mosi_data) });
    }
    if miso_len > 0 {
        // SAFETY: 10-bit field, max value 31 fits.
        s.miso_dlen().write(|w| unsafe { w.usr_miso_dbitlen().bits((miso_len - 1) as u16) });
    }

    // Kick off + wait for completion.
    s.cmd().modify(|_, w| w.usr().set_bit());
    while s.cmd().read().usr().bit_is_set() {
        core::hint::spin_loop();
    }

    // Restore prior register state.
    s.ctrl().write(|w| unsafe { w.bits(old_ctrl) });
    s.user().write(|w| unsafe { w.bits(old_user) });
    s.user1().write(|w| unsafe { w.bits(old_user1) });
    s.user2().write(|w| unsafe { w.bits(old_user2) });

    let mut ret = s.w0().read().bits();
    if miso_len < 32 {
        ret &= !(u32::MAX << miso_len);
    }
    ret
}

/// `bootloader_spi_flash_resume()` — send `CMD_RESUME` (0xAB) to wake the
/// chip from Deep Power Down. No-op if it was already awake.
#[inline]
unsafe fn spi_flash_resume() {
    execute_flash_command(CMD_RESUME, 0, 0, 0);
}

/// `bootloader_enable_wp()` — send `CMD_WRDI` (0x04) to re-enable the
/// flash write-protect after the unlock sequence. Source:
/// `bootloader_flash.c:677`.
#[inline]
unsafe fn enable_wp() {
    execute_flash_command(CMD_WRDI, 0, 0, 0);
}
