#![no_std]
#![no_main]

use esp_backtrace as _;
use esp_hal::{
    delay::Delay,
    dma::{Dma, DmaPriority},
    dma_tx_buffer,
    gpio::{Io, Level, Output},
    prelude::*,
    spi::{master::Spi, FullDuplexMode, SpiMode},
    Blocking,
};
mod hardware;
mod util;
extern crate alloc;
use core::mem::MaybeUninit;

fn init_heap() {
    const HEAP_SIZE: usize = 32 * 1024;
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

    esp_println::logger::init_logger_from_env();

    let io = Io::new(peripherals.GPIO, peripherals.IO_MUX);
    let mut led = Output::new(io.pins.gpio38, Level::Low);

    led.set_high();

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
    let spi: esp_hal::spi::master::SpiDma<
        '_,
        esp_hal::peripherals::SPI2,
        FullDuplexMode,
        Blocking,
    > = Spi::new(peripherals.SPI2, 40.MHz(), SpiMode::Mode0)
        .with_sck(sclk)
        .with_mosi(mosi)
        .with_dma(dma_channel.configure(false, DmaPriority::Priority9));

    let streamer =
        hardware::dma_streamer::DmaStreamer::new(spi, main_screen_buffer, spare_screen_buffer);
    let display_interface = hardware::dma_streamer::SpiDmaCMInterface::new(streamer, dc);
    let mut display = ili9341::Ili9341::new(
        display_interface,
        reset,
        &mut delay,
        ili9341::Orientation::LandscapeFlipped,
        ili9341::DisplaySize240x320,
    )
    .unwrap();
    ////
    display.clear_screen(0xf9b0).unwrap();
    loop {
        // log::info!("Hello world!");
        led.toggle();
        delay.delay(500.millis());
    }
}
