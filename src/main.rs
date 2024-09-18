#![feature(future_join)]
// use core::env;
use core::pin::pin;
use esp_idf_svc::{
    eventloop::EspSystemEventLoop,
    hal::{gpio::IOPin, peripherals::Peripherals, task::block_on},
    log::EspLogger,
    sys::{link_patches, EspError},
    timer::EspTaskTimerService,
};
use log::info;
use std::time::Duration;

mod display;
mod events;
mod inputs;
mod irq;

use display::{frame_timer, LedDisplay};
use inputs::InputManager;

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
    let mut inputs = InputManager::new().with_event_loop(sys_loop.clone());
    inputs.new_switch(peripherals.pins.gpio1.downgrade(), true)?;

    // Check the inputs via a timer circuit
    let input_timer = {
        let timer_service = EspTaskTimerService::new()?;
        timer_service.timer(move || {
            inputs.eval();
        })?
    };
    input_timer.every(Duration::from_millis(8))?;

    // Setup LED display
    let mut display = LedDisplay::new(peripherals.pins.gpio0, peripherals.rmt.channel0, 2).unwrap();

    // Set up a a timer to tick a display frame
    let frame_timer = frame_timer(sys_loop.clone())?;
    frame_timer.every(Duration::from_millis(16))?;

    // Main loop
    block_on(pin!(async move {
        let mut subscription = sys_loop.subscribe_async::<events::Event>()?;

        loop {
            let event = subscription.recv().await?;
            match event {
                events::Event::DisplayFrame => {
                    display.render_frame();
                }
                events::Event::Input(e) => {
                    info!("Input Event {:?}", e);
                }
            }
        }
    }))
}
