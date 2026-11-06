# Local AccessKit Windows memory fix

Upstream: `accesskit_windows` **0.35.2**, from <https://github.com/AccessKit/accesskit>.
The original adapter description is in `UPSTREAM-README.md`. This directory retains
the upstream MIT/Apache-2.0 licensing.

Local changes:

- Return the cached provider when removing it from `Context`.
- Queue that provider for disconnection when its node is removed or its role changes.
- After delivering queued accessibility events, call `UiaDisconnectProvider` outside
  the tree lock. Keep the window's root provider and active controls intact.

Without disconnection, an automated Studio page tour retained tens of thousands of
native UIA allocations. The removal-only prototype released 8.5 MiB of live heap
allocations. Windows documents the required lifecycle here:
<https://learn.microsoft.com/windows/win32/winauto/uiauto-serversideprovider#disconnecting-providers>.

`adapter::memory_tests::retired_control_cleanup_is_deferred_and_preserves_the_root`
regresses the ordinary-button removal path: it must queue cleanup and retain the
root provider's identity. Without the cleanup patch the queue is empty and the test fails.

Run: `cargo test --manifest-path crates/ui/vendor/accesskit_windows/Cargo.toml retired_control_cleanup --lib`.
