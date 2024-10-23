#![no_std]
#![no_main]

use alloc::boxed::Box;
use embedded_sdmmc::{SdCard, VolumeManager};
use esp_backtrace as _;
use esp_hal::{
    delay::Delay,
    dma::{Dma, DmaPriority},
    dma_tx_buffer,
    gpio::{Io, Level, Output},
    peripherals::TIMG0,
    prelude::*,
    spi::{master::Spi, SpiMode},
    timer::timg::{Timer, TimerX},
    Blocking,
};
use gameboy::{display::GameboyLineBufferDisplay, GameEmulationHandler, NullButtonHandler};
use gb_core::gameboy::GameBoy;
use hardware::{
    display::{
        dma_streamer::{DmaStreamer, SpiDmaCMInterface},
        scaler::ScreenScaler,
    },
    sound::NullAudioPlayer,
};
use ili9341::{DisplaySize, DisplaySize240x320};
mod gameboy;
mod hardware;
mod util;
extern crate alloc;
use core::mem::MaybeUninit;

fn init_heap() {
    const HEAP_SIZE: usize = 180_000;
    static mut HEAP: MaybeUninit<[u8; HEAP_SIZE]> = MaybeUninit::uninit();

    unsafe {
        esp_alloc::HEAP.add_region(esp_alloc::HeapRegion::new(
            HEAP.as_mut_ptr() as *mut u8,
            HEAP_SIZE,
            esp_alloc::MemoryCapability::Internal.into(),
        ));
    }
}

#[entry]
fn main() -> ! {
    #[allow(unused)]
    let peripherals = esp_hal::init(esp_hal::Config::default());
    let mut delay = Delay::new();

    init_heap();

    let timg0 = esp_hal::timer::timg::TimerGroup::new(peripherals.TIMG0);
    let timer0: Timer<TimerX<TIMG0, 0>, Blocking> = timg0.timer0;
    let timer1: Timer<TimerX<TIMG0, 1>, Blocking> = timg0.timer1;

    esp_println::logger::init_logger_from_env();

    let io = Io::new(peripherals.GPIO, peripherals.IO_MUX);
    let mut led = Output::new(io.pins.gpio47, Level::Low);

    led.set_high();

    const SCREEN_WIDTH: usize =
        (<DisplaySize240x320 as DisplaySize>::WIDTH as f32 / 1.0f32) as usize;
    const SCREEN_HEIGHT: usize =
        (<DisplaySize240x320 as DisplaySize>::HEIGHT as f32 / 1.0f32) as usize;

    log::info!("START ROM LOAD");
    /////////SDCARD
    let sclk = io.pins.gpio38;
    let miso = io.pins.gpio39;
    let mosi = io.pins.gpio37;
    let cs = Output::new(io.pins.gpio36, Level::Low);

    let spi = Spi::new(peripherals.SPI3, 200.kHz(), SpiMode::Mode0)
        .with_sck(sclk)
        .with_miso(miso)
        .with_mosi(mosi);

    let exclusive_spi = embedded_hal_bus::spi::ExclusiveDevice::new_no_delay(spi, cs).unwrap();
    let sdcard = SdCard::new(exclusive_spi, delay);

    let mut volume_mgr = VolumeManager::new(sdcard, hardware::sdcard::DummyTimesource::default());

    let mut volume0 = volume_mgr
        .open_volume(embedded_sdmmc::VolumeIdx(0))
        .unwrap();

    let mut root_dir = volume0.open_root_dir().unwrap();

    //Read boot rom
    let mut boot_rom_file = root_dir
        .open_file_in_dir("dmg_boot.bin", embedded_sdmmc::Mode::ReadOnly)
        .unwrap();
    let mut boot_rom_data = Box::new([0u8; 0x100]);
    boot_rom_file.read(&mut *boot_rom_data).unwrap();
    boot_rom_file.close().unwrap();

    let roms = gameboy::rom::SdRomManager::new("sml.gb", root_dir, Box::new(timer1));
    let gb_rom = gb_core::hardware::rom::Rom::from_bytes(roms);
    let cartridge = gb_rom.into_cartridge();

    let boot_rom = gb_core::hardware::boot_rom::Bootrom::new(Some(
        gb_core::hardware::boot_rom::BootromData::from_bytes(&*boot_rom_data),
    ));
    core::mem::drop(boot_rom_data);
    log::info!("ROM DATA LOADED");

    //////////SCREEN SETUP
    // ANCHOR: init-dma
    // we need to create the DMA driver and get a channel
    let dma = Dma::new(peripherals.DMA);
    let dma_channel = dma.channel0;

    let main_screen_buffer = dma_tx_buffer!(32000).unwrap();
    let spare_screen_buffer = dma_tx_buffer!(32000).unwrap();

    let mosi = io.pins.gpio4;
    let sclk = io.pins.gpio5;
    //
    let reset = Output::new(io.pins.gpio6, Level::Low);
    let dc = Output::new(io.pins.gpio7, Level::Low);

    // ANCHOR: configure-spi
    // we can call `.with_dma` on the SPI driver to make it use DMA
    let spi = Spi::new(peripherals.SPI2, 300.MHz(), SpiMode::Mode0)
        .with_sck(sclk)
        .with_mosi(mosi)
        .with_dma(dma_channel.configure(true, DmaPriority::Priority9));

    let streamer = DmaStreamer::new(spi, main_screen_buffer, spare_screen_buffer);
    let display_interface = SpiDmaCMInterface::new(streamer, dc);
    let mut display = ili9341::Ili9341::new(
        display_interface,
        reset,
        &mut delay,
        ili9341::Orientation::LandscapeFlipped,
        ili9341::DisplaySize240x320,
    )
    .unwrap();

    //GAMEBOY INIT
    let screen = GameboyLineBufferDisplay::new(Box::new(timer0));
    let mut gameboy = GameBoy::create(screen, cartridge, boot_rom, Box::new(NullAudioPlayer));
    let scaler: ScreenScaler<144, 160, { SCREEN_WIDTH }, { SCREEN_HEIGHT }> = ScreenScaler::new();
    ////
    display.clear_screen(0xf9b0).unwrap();

    let mut loop_counter: usize = 0;
    let mut sample_count: usize = 0;
    loop {
        // display.clear_screen(0xf9b0).unwrap();
        // display.clear_screen(0x423f).unwrap();
        // log::info!("Hello world!");
        let start_time = esp_hal::time::now();
        for x in (GameEmulationHandler::new(&mut gameboy, &mut NullButtonHandler)) {
            sample_count = sample_count.saturating_add(1);
        }
        // display
        //     .draw_raw_iter(
        //         0,
        //         0,
        //         (SCREEN_HEIGHT - 1) as u16,
        //         (SCREEN_WIDTH - 1) as u16,
        //         scaler.scale_iterator(GameEmulationHandler::new(
        //             &mut gameboy,
        //             &mut NullButtonHandler,
        //         )),
        //     )
        //     .unwrap();

        let end_time = esp_hal::time::now();
        let diff = end_time - start_time;
        let milliseconds = diff.to_millis();
        log::info!(
            "Loop: {}, Time elapsed: {}:{}",
            loop_counter,
            milliseconds / 1000,
            milliseconds % 1000
        );
        loop_counter += 1;
    }
}
