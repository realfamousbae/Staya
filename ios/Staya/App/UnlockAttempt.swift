/// A biometric result is valid once, and only for the current foreground attempt.
struct UnlockAttempt {
    private var serial: UInt64 = 0
    private var pending: UInt64?
    mutating func begin() -> UInt64 {
        serial &+= 1
        pending = serial
        return serial
    }
    mutating func invalidate() { pending = nil }
    mutating func finish(_ token: UInt64) -> Bool {
        guard pending == token else { return false }
        pending = nil
        return true
    }
}
