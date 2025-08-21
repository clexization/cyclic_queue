use std::sync::atomic::AtomicUsize;
use std::sync::atomic::Ordering::{Acquire, Release};

/// A cyclic queue which is optimized to be thread safe, if pop and
/// push are called from different threads at the same time without
/// locks and spinning locks. This is used in a microcontroller when
/// another component interrupts the main thread potentially between
/// the execution of a pop.
#[derive(Debug)]
pub struct CyclicQueue<T, const SIZE: usize> {
    queue: [Option<T>; SIZE],
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

impl<T, const SIZE: usize> CyclicQueue<T, SIZE> {
    /// Creates a new CyclicQueue with only empty fields
    pub const fn new() -> Self {
        assert!(SIZE > 0, "Queue size bigger than 0 expected");
        CyclicQueue {
            queue: [const { None }; SIZE],
            begin_index: AtomicUsize::new(0),
            end_index: AtomicUsize::new(0),
        }
    }

    /// The whole push operation can be assumed to happen atomic
    /// and is initiated by another thread.
    pub fn push(&mut self, value: T) -> Result<(), FullQueueError> {
        let end_index: usize = self.end_index.load(Acquire);
        let begin_index: usize = self.begin_index.load(Acquire);

        if is_capacity_reached(begin_index, end_index, SIZE) {
            return Err(FullQueueError);
        }

        self.queue[end_index % SIZE] = Some(value);

        self.end_index.store(end_index.wrapping_add(1), Release);

        Ok(())
    }

    /// Is running on the main application thread and may be
    /// interrupted at any possible point in time.
    pub fn pop(&mut self) -> Option<T> {
        let begin_index: usize = self.begin_index.load(Acquire);
        let end_index: usize = self.end_index.load(Acquire);

        if begin_index == end_index {  // is empty
            return None;
        }

        let data: Option<T> = self.queue[begin_index % SIZE].take();

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
    } else if begin_index < end_index {  // no wrapping and expected most of the time
        end_index - begin_index >= capacity
    } else {  // may happen due to wrapping increase
        let n_until_wrap = (usize::MAX - begin_index).wrapping_add(1);
        end_index + n_until_wrap >= capacity
    }
}


#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn single_push_pop() {
        let mut queue: CyclicQueue<u8, 10> = CyclicQueue::new();
        queue.push(1).unwrap();
        assert_eq!(Some(1), queue.pop());
    }

    #[test]
    fn pop_empty() {
        let mut queue: CyclicQueue<u8, 10> = CyclicQueue::new();
        assert_eq!(None, queue.pop());
    }

    #[test]
    fn push_full() {
        let mut queue: CyclicQueue<u8, 2> = CyclicQueue::new();
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
        let mut queue: CyclicQueue<u8, 10> = CyclicQueue::new();
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
        let mut queue: CyclicQueue<u8, 10> = CyclicQueue::new();
        queue.push(1).unwrap();
        assert!(!queue.is_full());
    }

    #[test]
    fn is_full_if_not_full_rolling() {
        let mut queue: CyclicQueue<u8, 10> = CyclicQueue::new();
        queue.begin_index.store(usize::MAX, Release);
        queue.end_index.store(usize::MAX, Release);

        queue.push(1).unwrap();

        assert!(!queue.is_full());
    }

    #[test]
    fn is_full_if_full() {
        let mut queue: CyclicQueue<u8, 1> = CyclicQueue::new();
        queue.push(1).unwrap();
        assert!(queue.is_full());
    }

    #[test]
    fn is_full_if_full_rolling() {
        let mut queue: CyclicQueue<u8, 2> = CyclicQueue::new();
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
        assert_eq!(queue_new.queue, queue_default.queue);
        assert_eq!(queue_new.begin_index.load(Acquire), queue_default.begin_index.load(Acquire));
        assert_eq!(queue_new.end_index.load(Acquire), queue_default.end_index.load(Acquire));
    }
}
