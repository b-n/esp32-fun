use esp_idf_svc::hal::{
    gpio::{AnyIOPin, Input as MODE_Input, InterruptType, Level, PinDriver, Pull},
    sys::EspError,
};
use log::{debug, error};
use std::collections::HashMap;

mod events;
mod irq;

pub use events::Event;
use irq::InterruptHandler;

// The number of samples to take when debouncing an input. When an input changes, an interrupt is
// fired. That interrupt is then cleared and checked during the input loop. It is quite likely that
// the input loop and the interrupt don't happen at the same time (e.g. all samples should be the
// same), however this provides a gaurantee of signal stability.
const SAMPLES: usize = 5;

// Help manage multiple inputs using interrupts that are debounced.
pub struct InputManager<'d> {
    inputs: HashMap<i32, Input<'d>>,
    irq_handler: InterruptHandler<'d>,
}

impl<'d> InputManager<'d> {
    // Generate a new input manager
    pub fn new() -> Self {
        Self {
            inputs: HashMap::with_capacity(32),
            irq_handler: InterruptHandler::new(),
        }
    }

    // Register an input with the input manager
    fn register_input(
        &mut self,
        pin: AnyIOPin,
        mode: InputMode,
        with_interrupts: bool,
    ) -> Result<(), EspError> {
        let mut input = Input::new(pin, mode)?;
        if with_interrupts {
            input = input.with_interrupts(&mut self.irq_handler)?
        }
        let pin = input.pin;
        self.inputs.insert(pin, input);
        Ok(())
    }

    // Helper function to register a switch type input
    #[allow(dead_code)]
    pub fn new_switch(&mut self, pin: AnyIOPin, with_interrupts: bool) -> Result<(), EspError> {
        self.register_input(pin, InputMode::Switch, with_interrupts)
    }

    // Helper function to register a button input
    // TODO: Support "Click" and "Double Click" events
    #[allow(dead_code)]
    pub fn new_button(&mut self, pin: AnyIOPin, with_interrupts: bool) -> Result<(), EspError> {
        self.register_input(pin, InputMode::Button, with_interrupts)
    }

    // Evalute the state of all inputs
    pub fn events(&mut self) -> Vec<Event> {
        let mut dequeued = 0;
        // Check the interrupt queue first and handle any messages
        while let Some(p) = self.irq_handler.dequeue() {
            if self.inputs.contains_key(&p) {
                self.inputs.get_mut(&p).unwrap().handle_interrupt().unwrap();
            } else {
                error!("Unhandled interrupt on pin {}", p);
            }
            dequeued += 1;
        }
        if dequeued > 0 {
            debug!("Dequeued {} interrupts", dequeued);
        }

        self.inputs
            .iter_mut()
            .fold(Vec::with_capacity(8), |mut acc, (_, input)| {
                if let Some(event) = input.tick() {
                    acc.push(event);
                }
                acc
            })
    }
}

pub enum InputMode {
    Switch,
    Button,
}

pub struct Input<'d> {
    pub state: Level,
    input: PinDriver<'d, AnyIOPin, MODE_Input>,
    pub pin: i32,
    pub dirty: bool,
    has_interrupts: bool,
    mode: InputMode,
}

impl<'d> Input<'d> {
    // Generate a new input
    pub fn new(pin: AnyIOPin, mode: InputMode) -> Result<Self, EspError> {
        let mut input = PinDriver::input(pin)?;
        let pin = input.pin();
        input.set_pull(Pull::Up)?;
        Ok(Self {
            state: input.get_level(),
            input,
            pin,
            dirty: false,
            has_interrupts: false,
            mode,
        })
    }

    // Register an interrupt handler for the input
    //
    // Note: this function is required at present since polling is not supported (yet)
    pub fn with_interrupts(mut self, handler: &mut InterruptHandler) -> Result<Self, EspError> {
        self.has_interrupts = true;
        // Setup the input pin
        self.input.set_interrupt_type(InterruptType::AnyEdge)?;
        unsafe { self.input.subscribe(handler.register(self.pin))? };
        self.input.enable_interrupt()?;

        Ok(self)
    }

    fn handle_interrupt(&mut self) -> Result<(), EspError> {
        if !self.has_interrupts {
            error!("Handling unregistered interrupt");
            // TODO: should be an error
            return Ok(());
        }
        // if we have an interrupt, we need to check the state of the input
        self.dirty = true;
        self.debounce();
        self.input.enable_interrupt()
    }

    // Debounce the input
    //
    // This function will debounce the input signal by ensuring that a signal has a constant level
    // for at least `SAMPLES` length. This is achieved in a O(1) memory space by starting a count
    // at `SAMPLES`, counting a HIGH as +1 and a LOW as -1. When the count reaches 0 or 2*SAMPLES,
    // then the signal should be stable for at least `SAMPLES` count.
    //
    // Warning: This function will indefinitely block if the signal is never stable (e.g.
    // floating). Ensure a pull-up or pull-down is set on the input
    fn debounce(&mut self) {
        let mut level = self.input.get_level();
        let mut count = SAMPLES;
        while count != 0 && count < SAMPLES * 2 {
            count = if level == Level::High {
                count.saturating_add(1)
            } else {
                count.saturating_sub(1)
            };
            level = self.input.get_level();
        }
        self.state = if count == 0 { Level::Low } else { Level::High };
    }

    // Evalute the state of the input, returning an input event if applicable.
    //
    // The state of the switch is debounced by taking a series of samples until
    // the window of samples are all the same value. The state is determined by
    // the final value of all samples combined (they need to be unanimous).
    //
    // Returns:
    // - None when nothing has changed
    // - Some(Event) based on the new state if it was changed
    fn tick(&mut self) -> Option<Event> {
        if !self.dirty {
            return None;
        }

        self.dirty = false;
        Some(self.input_event())
    }

    fn input_event(&self) -> Event {
        let pin = self.pin;
        match self.mode {
            InputMode::Switch => match self.state {
                Level::High => Event::On(pin),
                Level::Low => Event::Off(pin),
            },
            InputMode::Button => match self.state {
                Level::High => Event::Pressed(pin),
                Level::Low => Event::Released(pin),
            },
        }
    }
}
