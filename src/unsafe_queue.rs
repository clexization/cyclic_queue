use crate::FullQueueError;
use std::cell::UnsafeCell;
use std::sync::atomic::AtomicUsize;
use std::sync::atomic::Ordering::{Acquire, Release};

/// A cyclic queue which is optimized to be thread safe, if pop and
/// push are called from different threads at the same time without
/// locks and spinning locks. This is used in a microcontroller when
/// another component interrupts the main thread potentially between
/// the execution of a pop.

#[derive(Debug)]
pub(crate) struct UnsafeCyclicQueue<T, const SIZE: usize> {
    queue: UnsafeCell<[Option<T>; SIZE]>,
    begin_index: AtomicUsize,
    end_index: AtomicUsize,
}

impl<T, const SIZE: usize> Default for UnsafeCyclicQueue<T, SIZE> {
    fn default() -> Self {
        UnsafeCyclicQueue::new()
    }
}

/// These unsafe Sync/Send defeat somewhat the purpose of this
/// implementation, but the original use case was C++, where not
/// all guarantees of rust are available.
unsafe impl<T, const SIZE: usize> Sync for UnsafeCyclicQueue<T, SIZE> {}
unsafe impl<T, const SIZE: usize> Send for UnsafeCyclicQueue<T, SIZE> {}

impl<T, const SIZE: usize> UnsafeCyclicQueue<T, SIZE> {
    const ROLL_OVER: usize = usize::MAX - (usize::MAX % SIZE);

    /// Creates a new CyclicQueue with only empty fields
    pub(crate) const fn new() -> Self {
        assert!(SIZE > 0, "Queue size bigger than 0 expected");
        assert!(
            usize::MAX / SIZE > 1,
            "The size must fit into usize at least two times."
        );

        UnsafeCyclicQueue {
            queue: UnsafeCell::new([const { None }; SIZE]),
            begin_index: AtomicUsize::new(0),
            end_index: AtomicUsize::new(0),
        }
    }

    /// The whole push operation can be assumed to happen atomic
    /// and is initiated by another thread.
    /// unsafe: All calls of this method must happen on the same thread
    pub(crate) unsafe fn push(&self, value: T) -> Result<(), FullQueueError> {
        let end_index: usize = self.end_index.load(Acquire);
        let begin_index: usize = self.begin_index.load(Acquire);

        if Self::is_capacity_reached(begin_index, end_index) {
            return Err(FullQueueError);
        }

        (*self.queue.get())[end_index % SIZE] = Some(value);

        Self::increment_rollover(end_index, &self.end_index);

        Ok(())
    }

    /// Is running on the main application thread and may be
    /// interrupted at any possible point in time.
    /// unsafe: All calls of this method must happen on the same thread
    pub(crate) unsafe fn pop(&self) -> Option<T> {
        let begin_index: usize = self.begin_index.load(Acquire);
        let end_index: usize = self.end_index.load(Acquire);

        if begin_index == end_index {
            // is empty
            return None;
        }

        let data: Option<T> = (*self.queue.get())[begin_index % SIZE].take();

        Self::increment_rollover(begin_index, &self.begin_index);

        data
    }

    /// Check if at the current time the queue is empty
    pub(crate) fn is_empty(&self) -> bool {
        self.begin_index.load(Acquire) == self.end_index.load(Acquire)
    }

    /// Checks if at the current time the queue is full
    pub(crate) fn is_full(&self) -> bool {
        let begin_index: usize = self.begin_index.load(Acquire);
        let end_index: usize = self.end_index.load(Acquire);
        Self::is_capacity_reached(begin_index, end_index)
    }

    #[inline]
    fn increment_rollover(current_value: usize, atomic: &AtomicUsize) {
        atomic.store((current_value.wrapping_add(1)) % Self::ROLL_OVER, Release);
    }

    /// evaluation whether the capacity is reached or not
    #[inline]
    fn is_capacity_reached(begin_index: usize, end_index: usize) -> bool {
        (begin_index.wrapping_add(SIZE)) % Self::ROLL_OVER == end_index
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Arc, Barrier};
    use std::time::Instant;

    #[test]
    fn single_push_pop() {
        unsafe {
            let queue: UnsafeCyclicQueue<u8, 10> = UnsafeCyclicQueue::new();
            queue.push(1).unwrap();
            assert_eq!(Some(1), queue.pop());
        }
    }

    #[test]
    fn pop_empty() {
        unsafe {
            let queue: UnsafeCyclicQueue<u8, 10> = UnsafeCyclicQueue::new();
            assert_eq!(None, queue.pop());
        }
    }

    #[test]
    fn push_full() {
        unsafe {
            let queue: UnsafeCyclicQueue<u8, 2> = UnsafeCyclicQueue::new();
            queue.push(1).unwrap();
            queue.push(2).unwrap();
            assert_eq!(Err(FullQueueError), queue.push(3));
        }
    }

    #[test]
    fn is_empty_if_empty() {
        let queue: UnsafeCyclicQueue<u8, 10> = UnsafeCyclicQueue::new();
        assert!(queue.is_empty());
    }

    #[test]
    fn is_empty_if_not_empty() {
        unsafe {
            let queue: UnsafeCyclicQueue<u8, 10> = UnsafeCyclicQueue::new();
            queue.push(1).unwrap();
            assert!(!queue.is_empty());
        }
    }

    #[test]
    fn is_full_if_empty() {
        let queue: UnsafeCyclicQueue<u8, 10> = UnsafeCyclicQueue::new();
        assert!(!queue.is_full());
    }

    #[test]
    fn is_full_if_not_full() {
        unsafe {
            let queue: UnsafeCyclicQueue<u8, 10> = UnsafeCyclicQueue::new();
            queue.push(1).unwrap();
            assert!(!queue.is_full());
        }
    }

    #[test]
    fn is_full_if_not_full_rolling() {
        unsafe {
            let queue: UnsafeCyclicQueue<u8, 10> = UnsafeCyclicQueue::new();
            queue
                .begin_index
                .store(UnsafeCyclicQueue::<u8, 10>::ROLL_OVER - 1, Release);
            queue
                .end_index
                .store(UnsafeCyclicQueue::<u8, 10>::ROLL_OVER - 1, Release);

            queue.push(1).unwrap();

            assert!(!queue.is_full());
        }
    }

    #[test]
    fn is_full_if_full() {
        unsafe {
            let queue: UnsafeCyclicQueue<u8, 1> = UnsafeCyclicQueue::new();
            queue.push(1).unwrap();
            assert!(queue.is_full());
        }
    }

    #[test]
    fn is_full_if_full_rolling() {
        unsafe {
            let queue: UnsafeCyclicQueue<u8, 2> = UnsafeCyclicQueue::new();
            queue
                .begin_index
                .store(UnsafeCyclicQueue::<u8, 2>::ROLL_OVER - 1, Release);
            queue
                .end_index
                .store(UnsafeCyclicQueue::<u8, 2>::ROLL_OVER - 1, Release);

            queue.push(1).unwrap();
            queue.push(2).unwrap();

            assert!(queue.is_full());
        }
    }

    #[test]
    fn default_equal_new() {
        let queue_new: UnsafeCyclicQueue<u8, 10> = UnsafeCyclicQueue::new();
        let queue_default: UnsafeCyclicQueue<u8, 10> = UnsafeCyclicQueue::default();
        unsafe {
            assert_eq!(*queue_new.queue.get(), *queue_default.queue.get());
        }
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
        PUSH(Result<u32, FullQueueError>),
        POP(Option<u32>),
    }

    #[derive(Debug)]
    struct TimeStampData {
        time: Instant,
        data: AsyncResult,
    }

    impl TimeStampData {
        fn from_push(data: u32, result: Result<(), FullQueueError>) -> Self {
            TimeStampData {
                time: Instant::now(),
                data: AsyncResult::PUSH(result.map(|_| data)),
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
    fn example_implementation() {
        const TEST_SIZE: usize = 3;

        let push_queue = Arc::new(UnsafeCyclicQueue::<u32, TEST_SIZE>::new());
        let pop_queue = push_queue.clone();

        let barrier = Arc::new(Barrier::new(2));
        let push_barrier_ref = barrier.clone();
        let pop_barrier_ref = barrier.clone();

        let push_thread = std::thread::spawn(move || {
            let mut result: Vec<TimeStampData> = Vec::new();

            push_barrier_ref.wait();
            (0..20).for_each(|i| unsafe {
                result.push(TimeStampData::from_push(i, push_queue.push(i)))
            });

            result
        });

        let pop_thread = std::thread::spawn(move || {
            let mut result: Vec<TimeStampData> = Vec::new();

            pop_barrier_ref.wait();
            (0..40).for_each(|_| unsafe { result.push(TimeStampData::from_pop(pop_queue.pop())) });

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
