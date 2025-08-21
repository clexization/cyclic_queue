use std::cell::UnsafeCell;
use std::sync::atomic::AtomicUsize;
use std::sync::atomic::Ordering::{Acquire, Release};

/// A cyclic queue which is optimized to be thread safe, if pop and
/// push are called from different threads at the same time without
/// locks and spinning locks. This is used in a microcontroller when
/// another component interrupts the main thread potentially between
/// the execution of a pop.
#[derive(Debug)]
pub struct CyclicQueue<T, const SIZE: usize> {
    queue: UnsafeCell<[Option<T>; SIZE]>,
    begin_index: AtomicUsize,
    end_index: AtomicUsize,
}

/// Result in the case of a push onto a full queue
#[derive(Debug, PartialEq, Eq, Copy, Clone)]
pub struct FullQueueError;

impl<T, const SIZE: usize> Default for CyclicQueue<T, SIZE> {
    fn default() -> Self {
        CyclicQueue::new()
    }
}

/// These unsafe Sync/Send defeat somewhat the purpose of this
/// implementation, but the original use case was C++, where not
/// all guarantees of rust are available.
unsafe impl<T, const SIZE: usize> Sync for CyclicQueue<T, SIZE> {}
unsafe impl<T, const SIZE: usize> Send for CyclicQueue<T, SIZE> {}

impl<T, const SIZE: usize> CyclicQueue<T, SIZE> {
    /// Creates a new CyclicQueue with only empty fields
    pub const fn new() -> Self {
        assert!(SIZE > 0, "Queue size bigger than 0 expected");
        CyclicQueue {
            queue: UnsafeCell::new([const { None }; SIZE]),
            begin_index: AtomicUsize::new(0),
            end_index: AtomicUsize::new(0),
        }
    }

    /// The whole push operation can be assumed to happen atomic
    /// and is initiated by another thread.
    pub fn push(&self, value: T) -> Result<(), FullQueueError> {
        let end_index: usize = self.end_index.load(Acquire);
        let begin_index: usize = self.begin_index.load(Acquire);

        if is_capacity_reached(begin_index, end_index, SIZE) {
            return Err(FullQueueError);
        }

        unsafe {
            (*self.queue.get())[end_index % SIZE] = Some(value);
        }

        self.end_index.store(end_index.wrapping_add(1), Release);

        Ok(())
    }

    /// Is running on the main application thread and may be
    /// interrupted at any possible point in time.
    pub fn pop(&self) -> Option<T> {
        let begin_index: usize = self.begin_index.load(Acquire);
        let end_index: usize = self.end_index.load(Acquire);

        if begin_index == end_index {
            // is empty
            return None;
        }

        let data: Option<T>;
        unsafe {
            data = (*self.queue.get())[begin_index % SIZE].take();
        }

        self.begin_index.store(begin_index.wrapping_add(1), Release);

        data
    }

    /// Check if at the current time the queue is empty
    pub fn is_empty(&self) -> bool {
        self.begin_index.load(Acquire) == self.end_index.load(Acquire)
    }

    /// Checks if at the current time the queue is full
    pub fn is_full(&self) -> bool {
        let begin_index: usize = self.begin_index.load(Acquire);
        let end_index: usize = self.end_index.load(Acquire);
        is_capacity_reached(begin_index, end_index, SIZE)
    }
}

/// evaluation whether the capacity is reached or not
#[inline]
fn is_capacity_reached(begin_index: usize, end_index: usize, capacity: usize) -> bool {
    if begin_index == end_index {
        false
    } else if begin_index < end_index {
        // no wrapping and expected most of the time
        end_index - begin_index >= capacity
    } else {
        // may happen due to wrapping increase
        let n_until_wrap = (usize::MAX - begin_index).wrapping_add(1);
        end_index + n_until_wrap >= capacity
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Arc, Barrier};
    use std::time::Instant;

    #[test]
    fn single_push_pop() {
        let queue: CyclicQueue<u8, 10> = CyclicQueue::new();
        queue.push(1).unwrap();
        assert_eq!(Some(1), queue.pop());
    }

    #[test]
    fn pop_empty() {
        let queue: CyclicQueue<u8, 10> = CyclicQueue::new();
        assert_eq!(None, queue.pop());
    }

    #[test]
    fn push_full() {
        let queue: CyclicQueue<u8, 2> = CyclicQueue::new();
        queue.push(1).unwrap();
        queue.push(2).unwrap();
        assert_eq!(Err(FullQueueError), queue.push(3));
    }

    #[test]
    fn is_empty_if_empty() {
        let queue: CyclicQueue<u8, 10> = CyclicQueue::new();
        assert!(queue.is_empty());
    }

    #[test]
    fn is_empty_if_not_empty() {
        let queue: CyclicQueue<u8, 10> = CyclicQueue::new();
        queue.push(1).unwrap();
        assert!(!queue.is_empty());
    }

    #[test]
    fn is_full_if_empty() {
        let queue: CyclicQueue<u8, 10> = CyclicQueue::new();
        assert!(!queue.is_full());
    }

    #[test]
    fn is_full_if_not_full() {
        let queue: CyclicQueue<u8, 10> = CyclicQueue::new();
        queue.push(1).unwrap();
        assert!(!queue.is_full());
    }

    #[test]
    fn is_full_if_not_full_rolling() {
        let queue: CyclicQueue<u8, 10> = CyclicQueue::new();
        queue.begin_index.store(usize::MAX, Release);
        queue.end_index.store(usize::MAX, Release);

        queue.push(1).unwrap();

        assert!(!queue.is_full());
    }

    #[test]
    fn is_full_if_full() {
        let queue: CyclicQueue<u8, 1> = CyclicQueue::new();
        queue.push(1).unwrap();
        assert!(queue.is_full());
    }

    #[test]
    fn is_full_if_full_rolling() {
        let queue: CyclicQueue<u8, 2> = CyclicQueue::new();
        queue.begin_index.store(usize::MAX, Release);
        queue.end_index.store(usize::MAX, Release);

        queue.push(1).unwrap();
        queue.push(2).unwrap();

        assert!(queue.is_full());
    }

    #[test]
    fn default_equal_new() {
        let queue_new: CyclicQueue<u8, 10> = CyclicQueue::new();
        let queue_default: CyclicQueue<u8, 10> = CyclicQueue::default();
        assert_eq!(queue_new.queue.get(), queue_default.queue.get());
        assert_eq!(
            queue_new.begin_index.load(Acquire),
            queue_default.begin_index.load(Acquire)
        );
        assert_eq!(
            queue_new.end_index.load(Acquire),
            queue_default.end_index.load(Acquire)
        );
    }

    #[derive(Debug)]
    enum AsyncResult {
        PUSH(Result<(), FullQueueError>),
        POP(Option<u32>),
    }

    #[derive(Debug)]
    struct TimeStampData {
        time: Instant,
        data: AsyncResult,
    }

    impl TimeStampData {
        fn from_push(data: Result<(), FullQueueError>) -> Self {
            TimeStampData {
                time: Instant::now(),
                data: AsyncResult::PUSH(data),
            }
        }

        fn from_pop(data: Option<u32>) -> Self {
            TimeStampData {
                time: Instant::now(),
                data: AsyncResult::POP(data),
            }
        }
    }

    #[test]
    fn async_access() {
        let queue = Arc::new(CyclicQueue::<u32, 10>::new());

        let push_ref = queue.clone();
        let pop_ref = queue.clone();

        let barrier = Arc::new(Barrier::new(2));
        let push_barrier_ref = barrier.clone();
        let pop_barrier_ref = barrier.clone();

        let push_thread = std::thread::spawn(move || {
            let mut result: Vec<TimeStampData> = Vec::new();

            push_barrier_ref.wait();
            (0..10).for_each(|i| result.push(TimeStampData::from_push(push_ref.push(i))));

            result
        });

        let pop_thread = std::thread::spawn(move || {
            let mut result: Vec<TimeStampData> = Vec::new();

            pop_barrier_ref.wait();
            (0..30).for_each(|_| result.push(TimeStampData::from_pop(pop_ref.pop())));

            result
        });

        let mut result = push_thread.join().unwrap();
        result.append(&mut pop_thread.join().unwrap());

        result.sort_by(|a, b| a.time.partial_cmp(&b.time).unwrap());

        result.iter().for_each(|entry| match entry.data {
            AsyncResult::PUSH(data) => println!("{:?} Push: {:?}", entry.time, data),
            AsyncResult::POP(data) => println!("{:?} Pop: {:?}", entry.time, data),
        });
    }
}
