var attempt = UnlockAttempt()
let old = attempt.begin()
attempt.invalidate() // User backgrounds the app while the system dialog is open.
precondition(!attempt.finish(old), "Late success must not unlock after backgrounding")
let current = attempt.begin()
precondition(!attempt.finish(old), "Stale callback must not consume a new prompt")
precondition(attempt.finish(current))
precondition(!attempt.finish(current), "A result must only be accepted once")
print("Screen lock callback lifecycle passed")
