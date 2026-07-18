# ADR: authoritative loop and adapter runtime model

Status: accepted (2026-07-18)

## Decision

Keep the authoritative `SessionRuntime` synchronous and single-owner. Run TCP
transport on one dedicated Tokio runtime. Cross the boundary through bounded
command mailboxes, per-client bounded reliable replies, and a coalescing latest
observation watch channel.

Do not turn the game state into an async actor and do not create one runtime per
client.

## Compared models

| Model | Deterministic step boundary | Scheduling hops per step | Backpressure | Task growth |
| --- | --- | ---: | --- | --- |
| Selected hybrid | Direct `StepInput → Transition` call | 0 in Core/Session | Bounded commands/replies; coalesced observations | Constant server tasks plus two tasks per live connection |
| Fully async game actor | State hidden behind async request/reply | At least 2 | Requires another bounded mailbox and cancellation protocol | Constant actor tasks plus transport tasks |
| Thread per client | Direct state access requires locking or forwarding | Variable | OS/socket buffering dominates | One or more OS threads per connection |

The actor alternative adds scheduling, cancellation, shutdown, and reply-order
states without improving ownership: the current session already has exactly one
owner at the fixed-step composition root. A thread-per-client model has the
least predictable resource ceiling.

## Evidence and gates

- `session_command_batch_16ms` guards the complete authoritative transition.
- `transition_hash` guards causal replay/observation hashing.
- adapter stress tests cover disconnect storms, stalled senders, and 32-way
  observation fan-out.
- the end-to-end allocation gate includes a 100,000-transition soak with zero
  hot-path allocations.
- reliable queues are bounded; observations are latest-state coalescing, so a
  slow observer cannot grow memory with elapsed time.

This ADR is replaceable if measurements show that the synchronous boundary
misses its latency budget or if the product requires multiple independently
scheduled authoritative sessions in one process.
