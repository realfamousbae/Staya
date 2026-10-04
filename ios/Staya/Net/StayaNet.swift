import Foundation
import StayaCore

/// Единственный на процесс владелец сети к серверу Staya: клиент, `CoreSync` и
/// `queue`. Через эту очередь идут все изменения ядра, связанные с отправкой (в том
/// числе `prepareLocationUpdate`), сеть экрана (`LiveConnection`) и фоновая отправка
/// позиции — исходящая очередь ядра не обрабатывается параллельно.
@MainActor
final class StayaNet {
    static let shared = StayaNet()

    let queue = SerialQueue()
    private var core: StayaCore?
    private var bound: (client: StayaClient, sync: CoreSync)?

    /// `nil` — аккаунт ещё не привязан к серверу (онбординг не закончен).
    func bind(_ core: StayaCore) -> (client: StayaClient, sync: CoreSync)? {
        if self.core !== core || bound == nil {
            guard let client = try? StayaClient(core: core) else { return nil }
            self.core = core
            bound = (client, CoreSync(core: core, http: client))
        }
        return bound
    }
}
