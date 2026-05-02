//! Cache HAL initialization. Mirrors IDF v5.3 `cache_hal_init()` from
//! `idf_v53_ref/cache_hal.c` for the ESP32-P4 path.
//!
//! On P4 `CACHE_LL_LEVEL_EXT_MEM == 2` and `cache_ll_l1_enable_bus` is a
//! no-op (`cache_ll_p4.h:947` — "not used, for compatibility"), so the
//! whole `cache_hal_init` collapses to:
//!
//!   1. Read L2 autoload-enable bit from `L2_CACHE_AUTOLOAD_CTRL.ena`.
//!   2. Call ROM `Cache_Enable_L2_Cache(autoload ? 1 : 0)`.
//!
//! Note: the L2 cache *mode* (size, ways, line size) — set via
//! ROM `Cache_Set_L2_Cache_Mode` at `0x4FC003D4` — is configured
//! separately by `esp-p4-eth::try_new`. See memory note
//! `feedback_p4_cache_rom_hang.md` for why that call is needed under
//! `--ram --no-stub`.

#![allow(unsafe_code)]

/// `CACHE_LL_CACHE_AUTOLOAD` arg flag for the ROM `Cache_Enable_L2_Cache`.
/// Source: `cache_ll_p4.h` (the ROM treats non-zero as "preserve autoload").
const CACHE_LL_CACHE_AUTOLOAD: u32 = 1 << 0;

/// `Cache_Enable_L2_Cache(autoload_flag)` from `esp32p4.rom.ld:258`.
const ROM_CACHE_ENABLE_L2_CACHE: usize = 0x4FC0_0504;

/// `Cache_Set_L2_Cache_Mode(size, ways, line_size)` from `esp32p4.rom.ld`.
/// Must be called once after CPU reset, because the L2 controller's
/// internal mode/ways/line-size shadow registers are NOT re-initialised
/// by warm reset. IDF runs this from `cache_hal_init` in the second-stage
/// bootloader; our `--ram --no-stub` boot path skips it. Source:
/// `MIGRATION_PLAN/feedback_p4_cache_rom_hang.md` — without this call the
/// EMAC driver saw 77 % cold-boot reliability, with it 30/30 = 100 %.
const ROM_CACHE_SET_L2_CACHE_MODE: usize = 0x4FC0_03D4;

/// `Cache_Invalidate_All(map)` from `esp32p4.rom.ld`.
const ROM_CACHE_INVALIDATE_ALL: usize = 0x4FC0_0404;

/// `CACHE_MAP_L2_CACHE` flag for the ROM helpers — bit 5 selects the L2
/// cache. Source: `components/esp_rom/esp32p4/include/esp32p4/rom/cache.h`.
const CACHE_MAP_L2_CACHE: u32 = 1 << 5;

/// IDF default sdkconfig values for ESP32-P4: 256 KB / 8-way / 64 B line.
/// Enum values from `components/esp_rom/esp32p4/include/esp32p4/rom/cache.h`.
const CACHE_SIZE_256K: u32 = 10;
const CACHE_8WAYS_ASSOC: u32 = 2;
const CACHE_LINE_SIZE_64B: u32 = 3;

type RomCacheEnable = unsafe extern "C" fn(autoload_flag: u32);
type RomCacheSetL2Mode = unsafe extern "C" fn(size: u32, ways: u32, line_size: u32);
type RomCacheAll = unsafe extern "C" fn(map: u32) -> i32;

#[inline(always)]
fn cache() -> &'static esp32p4::cache::RegisterBlock {
    // SAFETY: CACHE::PTR is the PAC-provided MMIO base.
    unsafe { &*esp32p4::CACHE::PTR }
}

/// Mirror of IDF `cache_hal_init()` for the ESP32-P4 path.
///
/// Reads L2 autoload state from `L2_CACHE_AUTOLOAD_CTRL` and calls the
/// ROM `Cache_Enable_L2_Cache` with the corresponding flag, matching what
/// the IDF 2nd-stage bootloader does after MPLL is up.
pub fn hal_init() {
    let autoload = cache()
        .l2_cache_autoload_ctrl()
        .read()
        .l2_cache_autoload_ena()
        .bit();
    let flag = if autoload {
        CACHE_LL_CACHE_AUTOLOAD
    } else {
        0
    };

    // SAFETY: ROM symbol address is fixed in the P4 ROM image (rom.ld
    // v5.3, line 258). Calling convention is `void(uint32_t)`; we pass a
    // valid flag value (0 or 1).
    unsafe {
        let enable_l2: RomCacheEnable = core::mem::transmute(ROM_CACHE_ENABLE_L2_CACHE);
        enable_l2(flag);
    }
}

/// Mirror of `esp_p4_eth::dma::init_l2_cache_mode`. Calls ROM
/// `Cache_Set_L2_Cache_Mode(256K, 8way, 64B)` followed by
/// `Cache_Invalidate_All(L2)`. **Must be called once after CPU reset**
/// before any other L2 cache ROM helper — without this, helpers walk
/// the L2 with stale mode/ways/line-size state inherited from the
/// previous binary and intermittently hang the AHB bus, which on P4
/// surfaces as a periodic chip reset.
///
/// Phase-1 hypothesis: this is the missing step that turns the periodic
/// 5-10 ms POR into a stable boot.
pub fn init_l2_cache_mode() {
    // SAFETY: ROM addresses are fixed in P4 ROM. Single-hart at boot.
    unsafe {
        let set_mode: RomCacheSetL2Mode =
            core::mem::transmute(ROM_CACHE_SET_L2_CACHE_MODE);
        set_mode(CACHE_SIZE_256K, CACHE_8WAYS_ASSOC, CACHE_LINE_SIZE_64B);
        let inv_all: RomCacheAll = core::mem::transmute(ROM_CACHE_INVALIDATE_ALL);
        let _ = inv_all(CACHE_MAP_L2_CACHE);
    }
}
