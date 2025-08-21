# cyclic queue in rust

A problem I discussed with a colleague today involving a microcontroller constantly pushing
new values and each push might interrupt the main code execution to add new data to a queue.
The main code is regularly checking for new elements and is going to regularly pop from the
queue if new elements are available. In case of a full queue, new elements pushed are
rejected.