// Правила отправки позиции на Mac (CI):
//   swiftc -swift-version 6 ios/Staya/Location/LocationPolicy.swift ios/CoreTests/Location/main.swift -o loc && ./loc
import Foundation

func check(_ ok: Bool, _ what: String) {
    if !ok {
        print("FAIL: \(what)")
        exit(1)
    }
}

let t = Date(timeIntervalSince1970: 1_700_000_000.9)

let loc = LocationPolicy.toCore(Fix(latitude: 55.7558, longitude: 37.6173, accuracyM: 9.2, timestamp: t))
check(loc?.latE7 == 557_558_000 && loc?.lonE7 == 376_173_000, "coordinates")
check(loc?.accuracyM == 10, "accuracy rounded up")
check(loc?.timestamp == 1_700_000_000, "fix time")
check(LocationPolicy.toCore(Fix(latitude: -33.8, longitude: -70, accuracyM: 5, timestamp: t))?.latE7 == -338_000_000, "south")

check(LocationPolicy.toCore(Fix(latitude: 55, longitude: 37, accuracyM: -1, timestamp: t)) == nil, "invalid accuracy")
check(LocationPolicy.toCore(Fix(latitude: 55, longitude: 37, accuracyM: .nan, timestamp: t)) == nil, "nan accuracy")
check(LocationPolicy.toCore(Fix(latitude: 91, longitude: 37, accuracyM: 5, timestamp: t)) == nil, "latitude range")
check(LocationPolicy.toCore(Fix(latitude: 55, longitude: 181, accuracyM: 5, timestamp: t)) == nil, "longitude range")
check(LocationPolicy.toCore(Fix(latitude: .nan, longitude: 37, accuracyM: 5, timestamp: t)) == nil, "nan latitude")
check(LocationPolicy.toCore(Fix(latitude: 55, longitude: 37, accuracyM: 1e9, timestamp: t))?.accuracyM == 65_535, "clamp")

check(!"\(Fix(latitude: 55.7558, longitude: 37.6173, accuracyM: 5, timestamp: t))".contains("55.7"), "redacted")

// Свежесть: давно сохранённая точка не уходит; часы, спешащие вперёд, не мешают.
let fresh = Fix(latitude: 55, longitude: 37, accuracyM: 5, timestamp: t)
check(LocationPolicy.isFresh(fresh, now: t.addingTimeInterval(600)), "10 minutes is fresh")
check(!LocationPolicy.isFresh(fresh, now: t.addingTimeInterval(601)), "older than 10 minutes")
check(!LocationPolicy.isFresh(fresh, now: t.addingTimeInterval(35 * 3600)), "35 hours old")
check(LocationPolicy.isFresh(fresh, now: t.addingTimeInterval(-120)), "fix slightly in the future")

check(LocationPolicy.shouldSend(now: t, lastSent: nil), "first")
check(!LocationPolicy.shouldSend(now: t.addingTimeInterval(59.9), lastSent: t), "throttled")
check(LocationPolicy.shouldSend(now: t.addingTimeInterval(60), lastSent: t), "after a minute")
check(LocationPolicy.shouldSend(now: t.addingTimeInterval(-1), lastSent: t), "clock moved back")

// Придержанная точка уходит при остановке обновлений.
var throttle = SendThrottle()
let a = Fix(latitude: 55.70, longitude: 37.60, accuracyM: 5, timestamp: t)
let b = Fix(latitude: 55.71, longitude: 37.61, accuracyM: 5, timestamp: t.addingTimeInterval(20))
let c = Fix(latitude: 55.72, longitude: 37.62, accuracyM: 5, timestamp: t.addingTimeInterval(40))
check(throttle.offer(a, now: t)?.latitude == 55.70, "first sent")
check(throttle.offer(b, now: t.addingTimeInterval(20)) == nil, "b held")
check(throttle.offer(c, now: t.addingTimeInterval(40)) == nil, "c held")
check(throttle.takeHeld(now: t.addingTimeInterval(45))?.latitude == 55.72, "latest held goes out")
check(throttle.takeHeld(now: t.addingTimeInterval(46)) == nil, "held only once")
check(throttle.offer(a, now: t.addingTimeInterval(50)) == nil, "throttled after held send")
throttle.reset()
check(throttle.offer(a, now: t.addingTimeInterval(51)) != nil, "reset on enable")

// Ящик в фоне — не чаще раза в 5 минут; перевод часов назад не блокирует.
check(LocationPolicy.shouldPollMailbox(now: t, lastPoll: nil), "first mailbox poll")
check(!LocationPolicy.shouldPollMailbox(now: t.addingTimeInterval(299), lastPoll: t), "mailbox throttled")
check(LocationPolicy.shouldPollMailbox(now: t.addingTimeInterval(300), lastPoll: t), "mailbox after 5 min")
check(LocationPolicy.shouldPollMailbox(now: t.addingTimeInterval(-10), lastPoll: t), "clock moved back")

print("location: ok")
