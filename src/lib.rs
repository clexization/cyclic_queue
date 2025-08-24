use crate::unsafe_queue::UnsafeCyclicQueue;
use std::sync::atomic::AtomicBool;
use std::sync::atomic::Ordering::{Acquire, Relaxed, Release};
use std::sync::Arc;

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

/// This lockless cyclic queue only works with one [Producer] one
/// [Consumer]. Those are not required to be on the same thread and
/// may perform their actions in a thread safe manner.
///
/// # Example
/// ```
/// use cyclic_queue::*;
/// use std::sync::{Arc, Barrier};
///
/// let queue: Arc<CyclicQueue<i32, 2>> = CyclicQueue::new();
/// let mut producer = Producer::new(queue.clone()).unwrap();
/// let mut consumer = Consumer::new(queue.clone()).unwrap();
///
/// let producer_barrier = Arc::new(Barrier::new(2));
/// let consumer_barrier = producer_barrier.clone();
///
/// let producer_thread = std::thread::spawn(move || {
///     producer_barrier.wait();
///     (0..20).for_each(|i| {
///         println!("Push {} -> {:?}", i, producer.push(i));
///     });
/// });
///
/// let consumer_thread = std::thread::spawn(move || {
///     consumer_barrier.wait();
///     (0..40).for_each(|_| {
///         println!("Pop  {:?}", consumer.pop());
///     });
/// });
///
/// producer_thread.join().unwrap();
/// consumer_thread.join().unwrap();
/// ```
#[derive(Debug)]
pub struct CyclicQueue<T, const SIZE: usize> {
    queue: UnsafeCyclicQueue<T, SIZE>,
    has_producer: AtomicBool,
    has_consumer: AtomicBool,
}

unsafe impl<T, const SIZE: usize> Sync for CyclicQueue<T, SIZE> {}
unsafe impl<T, const SIZE: usize> Send for CyclicQueue<T, SIZE> {}

impl<T, const SIZE: usize> CyclicQueue<T, SIZE> {
    /// Creates a new [Arc](Arc)<[CyclicQueue](CyclicQueue)>. If a
    /// non reference is needed the [default](CyclicQueue::default)
    /// may be used instead. References are required for the
    /// [producer](Producer::new) and [consumer](Consumer::new).
    pub fn new() -> Arc<Self> {
        Arc::new(Self::default())
    }

    /// Whether this queue is currently empty. There is no reason
    /// to check for this before a [pop](Consumer::pop) as the
    /// [Consumer] will check for this itself.
    ///
    /// # Example
    /// ```
    /// use cyclic_queue::CyclicQueue;
    /// let queue = CyclicQueue::<u8, 5>::new();
    /// assert!(queue.is_empty());
    /// ```
    pub fn is_empty(&self) -> bool {
        self.queue.is_empty()
    }

    /// Whether this queue is currently full. There is no reason
    /// to check for this before a [push](Producer::push) as the
    /// [Producer] will check for this itself.
    ///
    /// # Example
    /// ```
    /// use cyclic_queue::{CyclicQueue, Producer};
    /// let queue = CyclicQueue::<u8, 1>::new();
    ///
    /// let mut producer = Producer::new(queue.clone()).unwrap();
    /// assert!(!queue.is_full());
    ///
    /// producer.push(0).unwrap();
    /// assert!(queue.is_full())
    /// ```
    pub fn is_full(&self) -> bool {
        self.queue.is_full()
    }
}

impl<T, const SIZE: usize> Default for CyclicQueue<T, SIZE> {
    fn default() -> Self {
        CyclicQueue {
            queue: UnsafeCyclicQueue::new(),
            has_producer: AtomicBool::new(false),
            has_consumer: AtomicBool::new(false),
        }
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

/// Each [CyclicQueue] may have only one producer at the time.
/// This producer may only be used on a single thread and is used to enqueue
/// new element for the [Consumer] in a lockless manner.
#[derive(Debug)]
pub struct Producer<T, const SIZE: usize> {
    cyclic_queue: Arc<CyclicQueue<T, SIZE>>,
}

unsafe impl<T, const SIZE: usize> Send for Producer<T, SIZE> {}

impl<T, const SIZE: usize> Producer<T, SIZE> {
    /// Creates a new producer for a [CyclicQueue] if no other producer
    /// exists at the same time. If another producer is currently in
    /// action, it has to be [dropped](Drop) first.
    ///
    /// # Example
    ///
    /// ```
    /// use cyclic_queue::{CyclicQueue, Producer};
    /// let queue = CyclicQueue::<u8, 5>::new();
    ///
    /// {
    ///     // first producer is allowed to exist
    ///     let producer = Producer::new(queue.clone());
    ///     assert!(producer.is_some());
    ///
    ///     // second producer cannot be created
    ///     assert!(Producer::new(queue.clone()).is_none());
    /// }
    ///
    /// // a new producer can be created, as the first one was dropped
    /// assert!(Producer::new(queue.clone()).is_some());
    /// ```
    pub fn new(queue: Arc<CyclicQueue<T, SIZE>>) -> Option<Self> {
        match queue
            .has_producer
            .compare_exchange(false, true, Acquire, Relaxed)
        {
            Ok(_) => Some(Producer {
                cyclic_queue: queue,
            }),
            _ => None,
        }
    }

    /// The push enqueues a new element to the [queue][Producer::queue]. This
    /// action is thread safe in combination with a [consumer](Consumer::pop).
    ///
    /// If the queue is [full](CyclicQueue::is_full), a [FullQueueError] will
    /// be returned.
    ///
    /// # Example
    ///
    /// ```
    /// use cyclic_queue::{CyclicQueue, FullQueueError, Producer};
    /// let queue = CyclicQueue::<u8, 1>::new();
    /// let mut producer = Producer::new(queue.clone()).unwrap();
    ///
    /// assert_eq!(Ok(()), producer.push(0));
    ///
    /// // queue is now full
    /// assert_eq!(Err(FullQueueError), producer.push(1));
    /// ```
    pub fn push(&mut self, value: T) -> Result<(), FullQueueError> {
        unsafe {
            // This is guaranteed to be only executed by this consumer reference
            self.cyclic_queue.queue.push(value)
        }
    }

    /// The reference for which [CyclicQueue] this producer is a reference for.
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

/// Each [CyclicQueue] may have only one consumer at the time.
/// This consumer may only be used on a single thread and is used to read
/// [enqueued](Producer) elements in a lockless manner.
#[derive(Debug)]
pub struct Consumer<T, const SIZE: usize> {
    cyclic_queue: Arc<CyclicQueue<T, SIZE>>,
}

unsafe impl<T, const SIZE: usize> Send for Consumer<T, SIZE> {}

impl<T, const SIZE: usize> Consumer<T, SIZE> {
    /// Creates a new consumer for a [CyclicQueue] if no other consumer
    /// exists at the same time. If another consumer is currently in
    /// action, it has to be [dropped](Drop) first.
    ///
    /// # Example
    ///
    /// ```
    /// use cyclic_queue::{CyclicQueue, Consumer};
    /// let queue = CyclicQueue::<u8, 5>::new();
    ///
    /// {
    ///     // first producer is allowed to exist
    ///     let consumer = Consumer::new(queue.clone());
    ///     assert!(consumer.is_some());
    ///
    ///     // second producer cannot be created
    ///     assert!(Consumer::new(queue.clone()).is_none());
    /// }
    ///
    /// // a new producer can be created, as the first one was dropped
    /// assert!(Consumer::new(queue.clone()).is_some());
    /// ```
    pub fn new(queue: Arc<CyclicQueue<T, SIZE>>) -> Option<Self> {
        match queue
            .has_consumer
            .compare_exchange(false, true, Acquire, Relaxed)
        {
            Ok(_) => Some(Consumer {
                cyclic_queue: queue,
            }),
            _ => None,
        }
    }

    /// The pop removes the oldest [enqueued](Producer::push) element from the
    /// [queue][Producer::queue]. This action is thread safe in combination with
    /// a [producer](Producer::push).
    ///
    /// If the queue is [empty](CyclicQueue::is_empty), a [None] will
    /// be returned.
    ///
    /// # Example
    ///
    /// ```
    /// use cyclic_queue::{Consumer, CyclicQueue, FullQueueError, Producer};
    /// let queue = CyclicQueue::<u8, 5>::new();
    /// let mut producer = Producer::new(queue.clone()).unwrap();
    /// let mut consumer = Consumer::new(queue.clone()).unwrap();
    ///
    /// assert_eq!(None, consumer.pop());
    /// producer.push(0).unwrap();
    /// producer.push(1).unwrap();
    ///
    /// assert_eq!(Some(0), consumer.pop());
    /// assert_eq!(Some(1), consumer.pop());
    /// assert_eq!(None, consumer.pop());
    /// ```
    pub fn pop(&mut self) -> Option<T> {
        unsafe { self.cyclic_queue.queue.pop() }
    }

    /// The reference for which [CyclicQueue] this producer is a reference for.
    pub fn queue(&self) -> &Arc<CyclicQueue<T, SIZE>> {
        &self.cyclic_queue
    }
}

impl<T, const SIZE: usize> Drop for Consumer<T, SIZE> {
    fn drop(&mut self) {
        self.cyclic_queue.has_consumer.store(false, Release);
    }
}
