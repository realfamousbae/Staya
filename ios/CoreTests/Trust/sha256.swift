import CryptoKit
import Foundation

/// SHA-256 для теста — независимо от реализации в ServerTrustEvaluator.
func sha256ForTest(_ bytes: UnsafeRawBufferPointer, digest: inout [UInt8]) {
    digest = Array(SHA256.hash(data: Data(bytes)))
}
