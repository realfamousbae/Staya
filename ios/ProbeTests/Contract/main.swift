// Контракт ProbeRecord ↔ tools/probe-server: каждое значение каждого перечисления
// и `null` в необязательных полях должны давать 204. Адрес и токен — из окружения.
import Foundation

let env = ProcessInfo.processInfo.environment
let url = URL(string: env["PROBE_URL"]!)!
let token = env["PROBE_TOKEN"]!

func post(_ r: ProbeRecord) async -> Int {
    var req = URLRequest(url: url)
    req.httpMethod = "POST"
    req.setValue("application/json", forHTTPHeaderField: "Content-Type")
    req.setValue("Bearer \(token)", forHTTPHeaderField: "Authorization")
    req.httpBody = try! JSONEncoder().encode(r)
    let (_, resp) = try! await URLSession.shared.data(for: req)
    return (resp as! HTTPURLResponse).statusCode
}

let base = ProbeRecord(
    device: "contract", strategy: "s4", eventTs: 1_700_000_000, trigger: .significantChange,
    appState: .relaunched, accuracyM: ProbeRecord.accuracy(65.4), speed: .init(metersPerSecond: 13.0),
    batteryPct: 77, charging: false, lowPower: true, prevSendMs: 512, prevSendFailures: 1,
    auth: .always, precise: false, bgRefresh: .available, eventsSinceLast: 4)
var nulls = base
nulls.accuracyM = ProbeRecord.accuracy(-1)
nulls.prevSendMs = nil
nulls.bgRefresh = nil
nulls.speed = .init(metersPerSecond: -1)

var cases: [(String, ProbeRecord)] = [("full", base), ("nulls", nulls)]
let triggers: [ProbeRecord.Trigger] = [.significantChange, .visit, .continuous, .timer, .motion, .foreground, .boot, .continuousStart, .continuousStop]
for t in triggers { var r = base; r.trigger = t; cases.append(("trigger \(t.rawValue)", r)) }
for a in [ProbeRecord.Auth.always, .whenInUse, .denied, .restricted, .notDetermined] { var r = base; r.auth = a; cases.append(("auth \(a.rawValue)", r)) }
for s in [ProbeRecord.Speed.unknown, .still, .walking, .driving] { var r = base; r.speed = s; cases.append(("speed \(s.rawValue)", r)) }
for b in [ProbeRecord.BgRefresh.available, .denied, .restricted] { var r = base; r.bgRefresh = b; cases.append(("bg \(b.rawValue)", r)) }
for st in [ProbeRecord.AppState.foreground, .background, .relaunched] { var r = base; r.appState = st; cases.append(("state \(st.rawValue)", r)) }

var failed = 0
for (name, r) in cases {
    let code = await post(r)
    if code != 204 { failed += 1; print("FAIL \(code) \(name)") }
}
print("contract: \(cases.count - failed)/\(cases.count) accepted")
exit(failed == 0 ? 0 : 1)
