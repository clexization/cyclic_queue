# single consumer and single producer lockless cyclic queue

## Background

A problem I discussed with a colleague today involving two microcontrollers. One controller
receives data and pushes it to another controller. On this other controller with a single core
the main loop is running. The main loop will regularly check for updates from a queue, which
requires to be safe from concurrency, as any update may happen on the same core due to an
interrupt triggered by the initial controller.

This is only a proof of concept as the original problem is written in C++. This is implemented
in Rust as a toy project to improve on the language. I do not know whether the assumed C++
microcontroller constraints hold true in the same microcontroller Rust implementation, but the
problem requiring a lockless solution seemed interesting enough.

## Algorithm

### Preconditions

The microcontroller has a single working core. All work on this single core could be stopped
at any given point of time by another independent signal, which calls a callback on the
working thread handling the data received from the received signal. This callback is guaranteed
to be not interrupted. After the callback is handled, the working core resumes to the last
action and continues. Due to this behavior a lockless solution is necessary.

The data which is received in the callback is enqueued into a queue, so that the working
core may use it at the required time. It is assumed that the queue is big enough, so that
newly received data will always be added. For the extremely rare case, that the queue is full,
the new element is allowed to be rejected.

While interrupts may happen at any given time, there is a guarantee, that `AtomicUsize` are
happening in an atomic fashion and cannot get interrupted.

### Non-working simple solution

This implementation uses an array to store the data. A start index indicates where the content
of the queue starts in this array and the end index indicates where the content ends.

The concurrency safety net here is that `push()` and `pop()` only modify one value once.

Here the code example (this is written for easier reading if unfamiliar with Rust):

```rust
use std::sync::atomic::AtomicUsize;
use std::sync::atomic::Ordering::{Acquire, Release};

struct Queue {
    array: [u8; 5],  // array of type u8 and size 5
    start_index: AtomicUsize,  // the element which is guaranteed to be atomic
    stop_index: AtomicUsize,
}

// The simplest solution would be the following:
impl Queue {
    fn push(&self, data: u8) {
        // check if full
        let start = self.start_index.load(Acquire);
        let stop = self.stop_index.load(Acquire);
        
        if Self::is_full(start, stop) {
            return;
        }
        
        self.array[self.stop_index] = data;
        self.stop_index.store((stop + 1) % 5, Release);
    }

    fn pop(&self) -> Option<u8> {
        // check if empty
        let start = self.start_index.load(Acquire);
        let stop = self.stop_index.load(Acquire);
        
        if Self::is_empty(start, stop) {
            return None;
        }
        
        let data = self.array[self.start_index];
        self.start_index.store((start + 1) % 5, Release);
        
        return Some(data);
    }
    
    fn is_empty(start: usize, stop: usize) -> bool {
        return start == stop;
    }
    
    fn is_full(start: usize, stop: usize) -> bool {
        return start == stop;
    }
}
```

As it can be seen, the `start == stop` is the same condition for the `is_empty()` and
`is_full()` due to the queue completing the cycle. A solution for this problem is
required, while still being safe from concurrency.

## Solution 1

To solve that `is_empty() == is_full()` one needs to be adjusted. This is done through the
changing the `is_full()` condition:

```rust
struct Queue {
    array: [u8; 6],  // To still have a queue of size 5, the index needs to be increased
    // ...
}

impl Queue {
    fn push(&self, data: u8) {
        // ...
        self.stop_index.store((stop + 1) % 6, Release);  // changed to new array size
    }

    fn pop(&self) -> Option<u8> {
        // ...
        self.start_index.store((start + 1) % 6, Release);  // changed to new array size
        // ...
    }
    
    fn is_full(start: usize, stop: usize) -> bool {
        return (start + 1) % 6 == stop;
    }
}
```

Now the cyclic queue will have an internal array with size `queu_size + 1` and one field
will always be empty. While this is an empty (useless) field, it still may be smaller
than adding any new atomic values to guarantee a concurrency issue.

If instead of an `u8` a way bigger structure is used, then this solution may be too space
inefficient for the use case.

## Solution 2

This repository implements this solution:

To solve the `is_empty() == is_full()` condition, the roll-over of the tracked indices is
adjusted, so that both states differentiate. This solution is quite optimal, if solution 1
is not applicable due to big data structures and the queue size is not needed to be too big.

Instead of using only the parts of the `start_index` and `stop_index` fitting into the
array size, the value transformation may also happen while reading and writing the data.
This requires the following assumption to be true: `2 * queue_size <= usize::MAX`.
If the index atomics can hold the size of the array twice, then the end condition may be
changed to `start_index + queue_size == end_index`.

In code, this looks like this:

```rust
impl Queue {
    fn push(&self, data: u8) {
        // ...
        self.array[self.stop_index % 5] = data;
        // needs to fit the size twice
        self.stop_index.store((stop + 1) % (2 * 5), Release);
    }

    fn pop(&self) -> Option<u8> {
        // ...
        let data = self.array[self.start_index % 5];
        // needs to fit the size twice
        self.start_index.store((start + 1) % (2 * 5), Release);
        // ...
    }
    
    fn is_full(start: usize, stop: usize) -> bool {
        return (start + 5) % (2 * 5) == stop;
    }
}
```