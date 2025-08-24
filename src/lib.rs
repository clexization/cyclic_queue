use std::sync::{Arc};
use std::sync::atomic::{AtomicBool};
use std::sync::atomic::Ordering::{Acquire, Relaxed, Release};
use crate::unsafe_queue::UnsafeCyclicQueue;

mod unsafe_queue;

/// Result in the case of a push onto a full queue
#[derive(Debug, PartialEq, Eq, Copy, Clone)]
pub struct FullQueueError;


//////////////////////////////////////////////////////////////////
//    _____           _ _         ____                          //
//   / ____|         | (_)       / __ \                         //
//  | |    _   _  ___| |_  ___  | |  | |_   _  ___ _   _  ___   //
//  | |   | | | |/ __| | |/ __| | |  | | | | |/ _ \ | | |/ _ \  //
//  | |___| |_| | (__| | | (__  | |__| | |_| |  __/ |_| |  __/  //
//   \_____\__, |\___|_|_|\___|  \___\_\\__,_|\___|\__,_|\___|  //
//          __/ |                                               //
//         |___/                                                //
//                                                              //
//////////////////////////////////////////////////////////////////

/// This lockless cyclic queue contains two parts: a producer and a consumer. These
/// parts may be used on the same or optimally on different parts. As the queu is
/// only thread safe for the case of pushes only being performed from one thread and
/// pops being performed from one thread.
#[derive(Debug)]
pub struct CyclicQueue<T, const SIZE: usize> {
    queue: UnsafeCyclicQueue<T, SIZE>,
    has_producer: AtomicBool,
    has_consumer: AtomicBool,
}

unsafe impl<T, const SIZE: usize> Sync for CyclicQueue<T, SIZE> {}
unsafe impl<T, const SIZE: usize> Send for CyclicQueue<T, SIZE> {}

impl<T, const SIZE: usize> CyclicQueue<T, SIZE> {
    pub const fn new() -> Self {
        CyclicQueue {
            queue: UnsafeCyclicQueue::new(),
            has_producer: AtomicBool::new(false),
            has_consumer: AtomicBool::new(false),
        }
    }

    pub fn is_empty(&self) -> bool {
        self.queue.is_empty()
    }

    pub fn is_full(&self) -> bool {
        self.queue.is_full()
    }
}

impl<T, const SIZE: usize> Default for CyclicQueue<T, SIZE> {
    fn default() -> Self {
        CyclicQueue::new()
    }
}


///////////////////////////////////////////////////
//   _____               _                       //
//  |  __ \             | |                      //
//  | |__) | __ ___   __| |_   _  ___ ___ _ __   //
//  |  ___/ '__/ _ \ / _` | | | |/ __/ _ \ '__|  //
//  | |   | | | (_) | (_| | |_| | (_|  __/ |     //
//  |_|   |_|  \___/ \__,_|\__,_|\___\___|_|     //
//                                               //
///////////////////////////////////////////////////

#[derive(Debug)]
pub struct Producer<T, const SIZE: usize> {
    cyclic_queue: Arc<CyclicQueue<T, SIZE>>,
}

unsafe impl<T, const SIZE: usize> Send for Producer<T, SIZE> {}

impl<T, const SIZE: usize> Producer<T, SIZE> {
    pub fn new(queue: Arc<CyclicQueue<T, SIZE>>) -> Option<Self> {
        match queue.has_producer.compare_exchange(false, true, Acquire, Relaxed) {
            Ok(_) => Some(Producer { cyclic_queue: queue }),
            _ => None,
        }
    }

    pub fn push(&mut self, value: T) -> Result<(), FullQueueError> {
        unsafe {
            // This is guaranteed to be only executed by this consumer reference
            self.cyclic_queue.queue.push(value)
        }
    }

    pub fn queue(&self) -> &Arc<CyclicQueue<T, SIZE>> {
        &self.cyclic_queue
    }
}

impl<T, const SIZE: usize> Drop for Producer<T, SIZE> {
    fn drop(&mut self) {
        self.cyclic_queue.has_producer.store(false, Release);
    }
}


////////////////////////////////////////////////////////
//    _____                                           //
//   / ____|                                          //
//  | |     ___  _ __  ___ _   _ _ __ ___   ___ _ __  //
//  | |    / _ \| '_ \/ __| | | | '_ ` _ \ / _ \ '__| //
//  | |___| (_) | | | \__ \ |_| | | | | | |  __/ |    //
//   \_____\___/|_| |_|___/\__,_|_| |_| |_|\___|_|    //
//                                                    //
////////////////////////////////////////////////////////

#[derive(Debug)]
pub struct Consumer<T, const SIZE: usize> {
    cyclic_queue: Arc<CyclicQueue<T, SIZE>>,
}

unsafe impl<T, const SIZE: usize> Send for Consumer<T, SIZE> {}

impl<T, const SIZE: usize> Consumer<T, SIZE> {
    pub fn new(queue: Arc<CyclicQueue<T, SIZE>>) -> Option<Self> {
        match queue.has_consumer.compare_exchange(false, true, Acquire, Relaxed) {
            Ok(_) => Some(Consumer { cyclic_queue: queue }),
            _ => None,
        }
    }

    pub fn pop(&mut self) -> Option<T> {
        unsafe { self.cyclic_queue.queue.pop() }
    }

    pub fn queue(&self) -> &Arc<CyclicQueue<T, SIZE>> {
        &self.cyclic_queue
    }
}

impl<T, const SIZE: usize> Drop for Consumer<T, SIZE> {
    fn drop(&mut self) {
        self.cyclic_queue.has_consumer.store(false, Release);
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Barrier;
    use super::*;

    #[test]
    fn example_implementation() {
        let queue = Arc::new(CyclicQueue::<i32, 5>::new());
        let mut producer = Producer::new(queue.clone()).unwrap();
        let mut consumer = Consumer::new(queue.clone()).unwrap();

        let barrier = Arc::new(Barrier::new(2));
        let push_barrier_ref = barrier.clone();
        let pop_barrier_ref = barrier.clone();

        let push_thread = std::thread::spawn(move || {
            push_barrier_ref.wait();

            (0..20).for_each(|i| {
                println!("Push {} -> {:?}", i, producer.push(i));
            });
        });

        let pop_thread = std::thread::spawn(move || {
            pop_barrier_ref.wait();

            (0..40).for_each(|_| {
                println!("Pop  {:?}", consumer.pop());
            });
        });

        push_thread.join().unwrap();
        pop_thread.join().unwrap();
    }
}