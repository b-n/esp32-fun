#![feature(future_join)]
// use core::env;
use core::pin::pin;
use esp_idf_svc::{
    eventloop::EspSystemEventLoop,
    hal::{delay, gpio::IOPin, peripherals::Peripherals, task::block_on},
    log::EspLogger,
    sys::{link_patches, EspError},
    timer::EspTaskTimerService,
};
use esp_inputs::{Event as InputEvent, InputManager};
use log::info;
use std::time::Duration;

mod events;
mod led_display;

use led_display::{frame_timer, LedDisplay};

// static NETWORK_SSID: &'static str = env!("NETWORK_SSID");
// static NETWORK_PW: &'static str = env!("NETWORK_PW");

fn main() -> Result<(), EspError> {
    // It is necessary to call this function once. Otherwise some patches to the runtime
    // implemented by esp-idf-sys might not link properly.
    // See https://github.com/esp-rs/esp-idf-template/issues/71
    link_patches();

    // Bind the log crate to the ESP Logging facilities
    EspLogger::initialize_default();

    let sys_loop = EspSystemEventLoop::take()?;
    let peripherals = Peripherals::take()?;

    // Setup input handlers
    let mut inputs = InputManager::new();
    inputs.new_switch(peripherals.pins.gpio5.downgrade(), true)?;
    inputs.new_switch(peripherals.pins.gpio6.downgrade(), true)?;
    inputs.new_switch(peripherals.pins.gpio7.downgrade(), true)?;
    inputs.new_switch(peripherals.pins.gpio8.downgrade(), true)?;
    inputs.new_switch(peripherals.pins.gpio9.downgrade(), true)?;
    inputs.new_switch(peripherals.pins.gpio10.downgrade(), true)?;
    inputs.new_switch(peripherals.pins.gpio20.downgrade(), true)?;
    inputs.new_switch(peripherals.pins.gpio21.downgrade(), true)?;

    // Check the inputs via a timer circuit
    let input_timer = {
        let timer_service = EspTaskTimerService::new()?;
        let sys_loop = sys_loop.clone();
        timer_service.timer(move || {
            for event in inputs.events() {
                sys_loop
                    .post::<events::Event>(&(event.into()), delay::BLOCK)
                    .unwrap();
            }
        })?
    };
    input_timer.every(Duration::from_millis(2))?;

    // Setup LED display
    let mut display = LedDisplay::new(peripherals.pins.gpio0, peripherals.rmt.channel0, 2).unwrap();

    // Set up a a timer to tick a display frame
    let frame_timer = frame_timer(sys_loop.clone())?;
    frame_timer.every(Duration::from_millis(16))?;

    // Main loop
    block_on(pin!(async move {
        let mut subscription = sys_loop.subscribe_async::<events::Event>()?;

        let mut bits: u8 = 0;

        loop {
            let event = subscription.recv().await?;
            match event {
                events::Event::DisplayFrame => {
                    display.render_frame();
                }
                events::Event::Input(e) => {
                    bits = match e {
                        InputEvent::On(gpio) => bits | gpio_to_bit_mask(gpio),
                        InputEvent::Off(gpio) => bits & !gpio_to_bit_mask(gpio),
                        _ => bits,
                    };

                    display.set_hue(bits);

                    info!("Input Event {:?}", e);
                }
            }
        }
    }))
}

fn gpio_to_bit_mask(gpio: i32) -> u8 {
    1 << match gpio {
        5 => 0,
        6 => 1,
        7 => 2,
        8 => 3,
        9 => 4,
        10 => 5,
        20 => 6,
        21 => 7,
        _ => 8, // overflows, but we don't care because it acts as a noop
    }
}
