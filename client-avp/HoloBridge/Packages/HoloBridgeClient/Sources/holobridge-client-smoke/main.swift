import Foundation
import HoloBridgeClientCore
import HoloBridgeClientTestAuth
import Darwin

@available(macOS 12.0, *)
private struct SmokeOptions {
    var host = "127.0.0.1"
    var port: UInt16 = 4433
    var durationSeconds: UInt64 = 5
    var allowInsecureCert = false
    var privateKeyPath = TestIdentityTokenSupplier.defaultPrivateKeyPath
    var testUserSub = "smoke-user-001"
    var requestVideo = false
    var resumeOnce = false
    var output: String?

    static func parse(_ arguments: ArraySlice<String>) throws -> SmokeOptions {
        var options = SmokeOptions()
        var iterator = arguments.makeIterator()

        while let argument = iterator.next() {
            switch argument {
            case "--host":
                options.host = try nextValue("--host", &iterator)
            case "--port":
                options.port = try parseUInt16("--port", &iterator)
            case "--duration-seconds":
                options.durationSeconds = try parseUInt64("--duration-seconds", &iterator)
            case "--allow-insecure-cert":
                options.allowInsecureCert = true
            case "--private-key-path":
                options.privateKeyPath = try nextValue("--private-key-path", &iterator)
            case "--test-user-sub":
                options.testUserSub = try nextValue("--test-user-sub", &iterator)
            case "--request-video":
                options.requestVideo = true
            case "--resume-once":
                options.resumeOnce = true
            case "--output":
                options.output = try nextValue("--output", &iterator)
            case "--help", "-h":
                throw SmokeUsage()
            default:
                throw SmokeError("unknown argument: \(argument)")
            }
        }

        return options
    }

    static func usage() -> String {
        """
        usage: holobridge-client-smoke [--host value] [--port value] [--duration-seconds value] [--allow-insecure-cert] [--private-key-path value] [--test-user-sub value] [--request-video] [--resume-once] [--output path]
        """
    }

    private static func nextValue(_ flag: String, _ iterator: inout ArraySlice<String>.Iterator) throws -> String {
        guard let value = iterator.next() else {
            throw SmokeError("\(flag) requires a value")
        }
        return value
    }

    private static func parseUInt16(
        _ flag: String,
        _ iterator: inout ArraySlice<String>.Iterator
    ) throws -> UInt16 {
        let value = try nextValue(flag, &iterator)
        guard let parsed = UInt16(value) else {
            throw SmokeError("\(flag) requires a valid UInt16 value")
        }
        return parsed
    }

    private static func parseUInt64(
        _ flag: String,
        _ iterator: inout ArraySlice<String>.Iterator
    ) throws -> UInt64 {
        let value = try nextValue(flag, &iterator)
        guard let parsed = UInt64(value) else {
            throw SmokeError("\(flag) requires a valid UInt64 value")
        }
        return parsed
    }
}

private struct SmokeUsage: Error {}
private struct SmokeError: LocalizedError {
    let detail: String
    init(_ detail: String) { self.detail = detail }
    var errorDescription: String? { detail }
}

@available(macOS 12.0, *)
private actor SmokeCollector {
    private var reassembler = H264VideoDatagramReassembler()
    private var completedAccessUnits = 0
    private var keyframes = 0
    private var totalBytes = 0
    private var receivedAfterDrop = 0
    private var droppedOnce = false
    private var outputHandle: FileHandle?

    func configureOutput(path: String?) throws {
        guard let path else {
            outputHandle = nil
            return
        }

        FileManager.default.createFile(atPath: path, contents: nil)
        outputHandle = try FileHandle(forWritingTo: URL(fileURLWithPath: path))
    }

    func consume(datagram: Data) throws {
        if let accessUnit = try reassembler.push(datagram: datagram) {
            completedAccessUnits += 1
            keyframes += accessUnit.isKeyframe ? 1 : 0
            totalBytes += accessUnit.data.count
            if droppedOnce {
                receivedAfterDrop += 1
            }
            try outputHandle?.write(contentsOf: accessUnit.data)
        }
    }

    func markDropTriggered() {
        droppedOnce = true
    }

    func summary() -> (Int, Int, Int, UInt64, Int) {
        (
            completedAccessUnits,
            keyframes,
            totalBytes,
            reassembler.stats.droppedIncompleteAccessUnits,
            receivedAfterDrop
        )
    }

    func closeOutput() throws {
        try outputHandle?.close()
    }
}

@available(macOS 12.0, *)
private actor StateTracker {
    private(set) var transitions: [SessionClientState] = []

    func record(_ state: SessionClientState) {
        transitions.append(state)
    }

    func snapshot() -> [SessionClientState] {
        transitions
    }
}

@available(macOS 12.0, *)
@main
enum SmokeMain {
    static func main() async {
        do {
            let options = try SmokeOptions.parse(CommandLine.arguments.dropFirst())
            try await run(options: options)
        } catch is SmokeUsage {
            print(SmokeOptions.usage())
            exit(EXIT_SUCCESS)
        } catch {
            fputs("error: \(error.localizedDescription)\n", stderr)
            fputs("\(SmokeOptions.usage())\n", stderr)
            exit(EXIT_FAILURE)
        }
    }

    private static func run(options: SmokeOptions) async throws {
        let endpoint = SessionEndpoint(host: options.host, port: options.port)
        let collector = SmokeCollector()
        try await collector.configureOutput(path: options.output)

        let tracker = StateTracker()
        let sessionClient = SessionClient(
            transportConfigurationFactory: { endpoint in
                TransportConfiguration(
                    host: endpoint.host,
                    port: endpoint.port,
                    serverName: "localhost",
                    allowInsecureCertificateValidation: options.allowInsecureCert
                )
            },
            onStateChange: { state in
                Task {
                    await tracker.record(state)
                }
            },
            onVideoDatagram: { datagram in
                Task {
                    try await collector.consume(datagram: datagram)
                }
            }
        )

        let tokenSupplier = TestIdentityTokenSupplier(
            subject: options.testUserSub,
            privateKeyPEMPath: options.privateKeyPath
        ).makeSupplier()

        let binding = try await sessionClient.connect(
            to: endpoint,
            identityTokenSupplier: tokenSupplier,
            requestVideo: options.requestVideo
        )

        print("hello_auth: success")
        print("session_id: \(binding.sessionID)")
        print("resume_token_issued: \(!binding.resumeToken.isEmpty)")
        print("resume_token_ttl_secs: \(binding.resumeTokenTTLSeconds)")

        let deadline = Date().addingTimeInterval(TimeInterval(options.durationSeconds))
        var didTriggerResume = false
        while Date() < deadline {
            try await Task.sleep(nanoseconds: 100_000_000)

            if options.resumeOnce, !didTriggerResume {
                let summary = await collector.summary()
                if summary.0 > 0 {
                    didTriggerResume = true
                    await collector.markDropTriggered()
                    await sessionClient.simulateNetworkDrop()
                }
            }
        }

        let currentBinding = await sessionClient.binding
        let stateTransitions = await tracker.snapshot()
        let summary = await collector.summary()
        try await collector.closeOutput()

        print("completed_access_units: \(summary.0)")
        print("keyframes: \(summary.1)")
        print("total_bytes: \(summary.2)")
        print("dropped_incomplete_access_units: \(summary.3)")
        if options.resumeOnce {
            print("resume_result: \(summary.4 > 0 ? "success" : "failed")")
        }
        print("final_state: \(String(describing: await sessionClient.state))")
        if let currentBinding {
            print("current_session_id: \(currentBinding.sessionID)")
            print("current_resume_token_issued: \(!currentBinding.resumeToken.isEmpty)")
        }
        print("state_transitions: \(stateTransitions.map(String.init(describing:)).joined(separator: " -> "))")

        await sessionClient.disconnect(reason: "smoke-complete")
    }
}
