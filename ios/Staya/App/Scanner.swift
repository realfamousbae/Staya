import SwiftUI
import VisionKit

/// Сканер QR-кода приглашения (VisionKit): находит `staya://…` один раз.
/// Кадры нигде не сохраняются. Без поддержки на устройстве — вставка ссылки.
struct QrScanner: UIViewControllerRepresentable {
    let onResult: (DeepLink) -> Void

    static var isAvailable: Bool {
        DataScannerViewController.isSupported && DataScannerViewController.isAvailable
    }

    func makeUIViewController(context: Context) -> DataScannerViewController {
        let scanner = DataScannerViewController(
            recognizedDataTypes: [.barcode(symbologies: [.qr])],
            qualityLevel: .balanced,
            isHighlightingEnabled: true
        )
        scanner.delegate = context.coordinator
        try? scanner.startScanning()
        return scanner
    }

    func updateUIViewController(_ controller: DataScannerViewController, context: Context) {}

    static func dismantleUIViewController(_ controller: DataScannerViewController, coordinator: Coordinator) {
        controller.stopScanning()
    }

    func makeCoordinator() -> Coordinator { Coordinator(onResult: onResult) }

    @MainActor
    final class Coordinator: NSObject, DataScannerViewControllerDelegate {
        let onResult: (DeepLink) -> Void
        private var done = false

        init(onResult: @escaping (DeepLink) -> Void) { self.onResult = onResult }

        func dataScanner(_ scanner: DataScannerViewController, didAdd items: [RecognizedItem], allItems: [RecognizedItem]) {
            guard !done else { return }
            for item in items {
                if case .barcode(let code) = item, let link = DeepLink.parse(code.payloadStringValue) {
                    done = true
                    scanner.stopScanning()
                    onResult(link)
                    return
                }
            }
        }
    }
}
