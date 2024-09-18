use heapless::spsc::{Consumer, Queue};

const IRQ_CAPACITY: usize = 8;

type PinAddress = i32;

pub static mut Q: Queue<i32, IRQ_CAPACITY> = Queue::new();

pub struct InterruptHandler<'h> {
    consumer: Consumer<'h, PinAddress, IRQ_CAPACITY>,
}

impl<'h> InterruptHandler<'h> {
    pub fn new() -> Self {
        let irq_consumer = unsafe { Q.split().1 };
        InterruptHandler {
            consumer: irq_consumer,
        }
    }

    pub fn register(&mut self, pin: PinAddress) -> impl FnMut() {
        move || {
            let mut producer = unsafe { Q.split().0 };
            producer.enqueue(pin).ok().unwrap();
        }
    }

    pub fn dequeue(&mut self) -> Option<PinAddress> {
        self.consumer.dequeue()
    }
}
