/// Optimized Mouse Handling for macOS
///
/// Implement Rate Limiting (Coalescing) to reduce network jitter.
///
/// Recommended Strategy:
/// 1. Use a designated "update interval" (e.g., 8ms for ~120Hz).
/// 2. Accumulate delta movement (dx, dy) between updates.
/// 3. Ignore intermediate absolute positions if using deltas.
/// 4. Only send the final accumulated packet at the end of the interval.
///
/// Pseudocode Implementation (for MacosInputSource):
///
/// ```rust
/// static LAST_MOUSE_SEND: Mutex<Instant> = Mutex::new(Instant::now());
/// static ACCUMULATED_X: Mutex<f32> = Mutex::new(0.0);
/// static ACCUMULATED_Y: Mutex<f32> = Mutex::new(0.0);
///
/// fn handle_mouse_move(event) {
///     let now = Instant::now();
///     
///     // Accumulate
///     *ACCUMULATED_X.lock().unwrap() += event.dx;
///     *ACCUMULATED_Y.lock().unwrap() += event.dy;
///
///     if now.duration_since(*LAST_MOUSE_SEND.lock().unwrap()) >= Duration::from_millis(8) {
///          // Send Packet
///          send_network_event(x, y);
///          
///          // Reset
///          *LAST_MOUSE_SEND.lock().unwrap() = now;
///          *ACCUMULATED_X = 0;
///          *ACCUMULATED_Y = 0;
///     } else {
///          // Drop Event (Coalesce)
///          return; 
///     }
/// }
/// ```
///
/// Note: Ensure `TCP_NODELAY` is enabled on the socket via `set_nodelay(true)`.
