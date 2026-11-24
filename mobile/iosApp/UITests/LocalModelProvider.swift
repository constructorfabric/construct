import Foundation
import Network

/// An OpenAI-compatible test fixture listening only on loopback in the iOS simulator.
final class LocalModelProvider {
    enum Reply {
        case json(Int, String, pause: TimeInterval = 0)
        case stream(String, pause: TimeInterval? = nil)
    }

    private let listener: NWListener
    private let queue = DispatchQueue(label: "test.model-provider")
    private let key: String
    private let model: String
    private var replies: ArraySlice<Reply>
    private var connections: [ObjectIdentifier: NWConnection] = [:]

    init(key: String, model: String, replies: [Reply], ready: @escaping (UInt16) -> Void) throws {
        self.key = key
        self.model = model
        self.replies = replies[...]
        let parameters = NWParameters.tcp
        parameters.requiredLocalEndpoint = .hostPort(host: "127.0.0.1", port: .any)
        listener = try NWListener(using: parameters)
        listener.stateUpdateHandler = { [weak listener] state in
            if case .ready = state, let port = listener?.port {
                ready(port.rawValue)
            }
        }
        listener.newConnectionHandler = { [weak self] connection in
            self?.accept(connection)
        }
        listener.start(queue: queue)
    }

    func close() {
        queue.sync {
            listener.cancel()
            for connection in connections.values { connection.cancel() }
            connections.removeAll()
        }
    }

    private func accept(_ connection: NWConnection) {
        connections[ObjectIdentifier(connection)] = connection
        connection.start(queue: queue)
        read(connection, data: Data())
    }

    private func read(_ connection: NWConnection, data: Data) {
        connection.receive(minimumIncompleteLength: 1, maximumLength: 64 * 1024) { [weak self] chunk, _, ended, error in
            guard let self else { return }
            let received = data + (chunk ?? Data())
            if let request = ModelRequest(received) {
                self.send(self.reply(to: request), to: connection)
            } else if !ended && error == nil && received.count < 256 * 1024 {
                self.read(connection, data: received)
            } else {
                self.finish(connection)
            }
        }
    }

    private func reply(to request: ModelRequest) -> Reply {
        guard request.authorization == "Bearer \(key)" else {
            return .json(401, #"{"error":{"message":"API key refused"}}"#)
        }
        guard request.model == model else {
            return .json(404, #"{"error":{"message":"No such model"}}"#)
        }
        return replies.popFirst() ?? .json(500, "{}")
    }

    private func send(_ reply: Reply, to connection: NWConnection) {
        switch reply {
        case let .json(status, body, pause):
            let headers = "HTTP/1.1 \(status) Test\r\nContent-Type: application/json\r\n"
                + "Content-Length: \(body.utf8.count)\r\nConnection: close\r\n\r\n"
            queue.asyncAfter(deadline: .now() + pause) { [weak self] in
                connection.send(content: Data((headers + body).utf8), completion: .contentProcessed { _ in
                    self?.finish(connection)
                })
            }
        case let .stream(text, pause):
            let headers = "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nConnection: close\r\n\r\n"
            let event = ["choices": [["delta": ["content": text]]]]
            guard let encoded = try? JSONSerialization.data(withJSONObject: event) else {
                finish(connection)
                return
            }
            let delta = Data((headers + "data: ").utf8) + encoded + Data("\n\n".utf8)
            connection.send(content: delta, completion: .contentProcessed { _ in })
            guard let pause else {
                keepOpen(connection)
                return
            }
            queue.asyncAfter(deadline: .now() + pause) { [weak self] in
                let end = #"data: {"choices":[{"delta":{},"finish_reason":"stop"}]}"# + "\n\ndata: [DONE]\n\n"
                connection.send(content: Data(end.utf8), completion: .contentProcessed { _ in
                    self?.finish(connection)
                })
            }
        }
    }

    // Hold the stream until the app closes it on Stop or the test tears the fixture down.
    private func keepOpen(_ connection: NWConnection) {
        queue.asyncAfter(deadline: .now() + 1) { [weak self] in
            guard let self, self.connections[ObjectIdentifier(connection)] != nil else { return }
            connection.send(content: Data(": waiting\n\n".utf8), completion: .contentProcessed { error in
                if error == nil {
                    self.keepOpen(connection)
                } else {
                    self.finish(connection)
                }
            })
        }
    }

    private func finish(_ connection: NWConnection) {
        connection.cancel()
        connections.removeValue(forKey: ObjectIdentifier(connection))
    }
}

private struct ModelRequest {
    let authorization: String?
    let model: String?

    init?(_ data: Data) {
        let boundary = Data("\r\n\r\n".utf8)
        guard let range = data.range(of: boundary),
              let headerText = String(bytes: data[..<range.lowerBound], encoding: .utf8) else { return nil }
        let headers = headerText.components(separatedBy: "\r\n")
        let length = Self.header("content-length", in: headers).flatMap(Int.init) ?? 0
        let body = data[range.upperBound...]
        guard body.count >= length else { return nil }
        authorization = Self.header("authorization", in: headers)
        let json = (try? JSONSerialization.jsonObject(with: Data(body.prefix(length)))) as? [String: Any]
        model = json?["model"] as? String
    }

    private static func header(_ name: String, in headers: [String]) -> String? {
        for line in headers {
            let fields = line.split(separator: ":", maxSplits: 1).map { $0.trimmingCharacters(in: .whitespaces) }
            if fields.count == 2 && fields[0].lowercased() == name { return fields[1] }
        }
        return nil
    }
}
