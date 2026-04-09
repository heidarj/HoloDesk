import HoloBridgeClientCore
import SwiftUI
import UIKit

struct RemoteInputCaptureView: UIViewRepresentable {
    let session: SessionManager
    let videoPixelSize: CGSize

    func makeUIView(context: Context) -> RemoteInputCaptureUIView {
        let view = RemoteInputCaptureUIView()
        view.backgroundColor = .clear
        return view
    }

    func updateUIView(_ uiView: RemoteInputCaptureUIView, context: Context) {
        uiView.session = session
        uiView.videoPixelSize = videoPixelSize
        uiView.isInputEnabled = session.state.isConnected && !session.remoteInputSuppressed
        uiView.activateKeyboardFocusIfNeeded()
    }
}

final class RemoteInputCaptureUIView: UIView, UIGestureRecognizerDelegate {
    weak var session: SessionManager?
    var videoPixelSize: CGSize = .zero
    var isInputEnabled = false

    private var wheelAccumulatorX: CGFloat = 0
    private var wheelAccumulatorY: CGFloat = 0
    private var primaryPressActive = false

    private lazy var primaryPressRecognizer: UILongPressGestureRecognizer = {
        let recognizer = UILongPressGestureRecognizer(
            target: self,
            action: #selector(handlePrimaryPress(_:))
        )
        recognizer.minimumPressDuration = 0
        recognizer.allowableMovement = .greatestFiniteMagnitude
        recognizer.delegate = self
        return recognizer
    }()

    private lazy var scrollRecognizer: UIPanGestureRecognizer = {
        let recognizer = UIPanGestureRecognizer(
            target: self,
            action: #selector(handleScroll(_:))
        )
        recognizer.minimumNumberOfTouches = 2
        recognizer.maximumNumberOfTouches = 2
        recognizer.delegate = self
        return recognizer
    }()

    private lazy var hoverRecognizer: UIHoverGestureRecognizer = {
        let recognizer = UIHoverGestureRecognizer(
            target: self,
            action: #selector(handleHover(_:))
        )
        recognizer.delegate = self
        return recognizer
    }()

    override init(frame: CGRect) {
        super.init(frame: frame)
        isOpaque = false
        addGestureRecognizer(primaryPressRecognizer)
        addGestureRecognizer(scrollRecognizer)
        addGestureRecognizer(hoverRecognizer)
    }

    @available(*, unavailable)
    required init?(coder: NSCoder) {
        fatalError("init(coder:) has not been implemented")
    }

    override var canBecomeFirstResponder: Bool {
        true
    }

    override func didMoveToWindow() {
        super.didMoveToWindow()
        activateKeyboardFocusIfNeeded()
    }

    func activateKeyboardFocusIfNeeded() {
        guard window != nil else {
            return
        }

        DispatchQueue.main.async { [weak self] in
            _ = self?.becomeFirstResponder()
        }
    }

    override func pressesBegan(
        _ presses: Set<UIPress>,
        with event: UIPressesEvent?
    ) {
        let handled = forwardKeyPresses(presses, phase: "down")
        if !handled {
            super.pressesBegan(presses, with: event)
        }
    }

    override func pressesEnded(
        _ presses: Set<UIPress>,
        with event: UIPressesEvent?
    ) {
        let handled = forwardKeyPresses(presses, phase: "up")
        if !handled {
            super.pressesEnded(presses, with: event)
        }
    }

    override func pressesCancelled(
        _ presses: Set<UIPress>,
        with event: UIPressesEvent?
    ) {
        let handled = forwardKeyPresses(presses, phase: "up")
        if !handled {
            super.pressesCancelled(presses, with: event)
        }
    }

    func gestureRecognizer(
        _ gestureRecognizer: UIGestureRecognizer,
        shouldRecognizeSimultaneouslyWith otherGestureRecognizer: UIGestureRecognizer
    ) -> Bool {
        true
    }

    @objc
    private func handleHover(_ recognizer: UIHoverGestureRecognizer) {
        guard isInputEnabled else {
            return
        }

        let point = recognizer.location(in: self)
        guard let mapped = desktopPoint(for: point) else {
            return
        }
        session?.sendPointerMotion(x: mapped.x, y: mapped.y)
    }

    @objc
    private func handlePrimaryPress(_ recognizer: UILongPressGestureRecognizer) {
        let point = recognizer.location(in: self)

        guard let mapped = desktopPoint(for: point) else {
            if primaryPressActive {
                primaryPressActive = false
            }
            return
        }

        switch recognizer.state {
        case .began:
            guard isInputEnabled else {
                return
            }
            primaryPressActive = true
            session?.sendPointerButton(
                button: "left",
                phase: "down",
                x: mapped.x,
                y: mapped.y
            )
        case .changed:
            guard isInputEnabled, primaryPressActive else {
                return
            }
            session?.sendPointerMotion(x: mapped.x, y: mapped.y)
        case .ended:
            guard primaryPressActive else {
                return
            }
            primaryPressActive = false
            session?.sendPointerButton(
                button: "left",
                phase: "up",
                x: mapped.x,
                y: mapped.y
            )
        case .cancelled, .failed:
            guard primaryPressActive else {
                return
            }
            primaryPressActive = false
            session?.sendPointerButton(
                button: "left",
                phase: "up",
                x: mapped.x,
                y: mapped.y
            )
        default:
            break
        }
    }

    @objc
    private func handleScroll(_ recognizer: UIPanGestureRecognizer) {
        guard isInputEnabled else {
            recognizer.setTranslation(.zero, in: self)
            wheelAccumulatorX = 0
            wheelAccumulatorY = 0
            return
        }

        let point = recognizer.location(in: self)
        guard let mapped = desktopPoint(for: point) else {
            return
        }

        switch recognizer.state {
        case .began, .changed:
            let delta = recognizer.translation(in: self)
            recognizer.setTranslation(.zero, in: self)
            wheelAccumulatorX += delta.x * 24
            wheelAccumulatorY -= delta.y * 24

            let wheelX = consumeWheelDelta(&wheelAccumulatorX)
            let wheelY = consumeWheelDelta(&wheelAccumulatorY)
            guard wheelX != 0 || wheelY != 0 else {
                return
            }

            session?.sendWheel(
                deltaX: Int32(wheelX),
                deltaY: Int32(wheelY),
                x: mapped.x,
                y: mapped.y
            )
        case .ended, .cancelled, .failed:
            wheelAccumulatorX = 0
            wheelAccumulatorY = 0
        default:
            break
        }
    }

    private func forwardKeyPresses(
        _ presses: Set<UIPress>,
        phase: String
    ) -> Bool {
        guard isInputEnabled else {
            return false
        }

        var handled = false
        for press in presses {
            guard let key = press.key else {
                continue
            }
            handled = true
            session?.sendKey(
                keyCode: UInt16(key.keyCode.rawValue),
                phase: phase,
                modifiers: UInt32(key.modifierFlags.rawValue)
            )
        }
        return handled
    }

    private func desktopPoint(for point: CGPoint) -> (x: Int32, y: Int32)? {
        guard
            bounds.width > 0,
            bounds.height > 0,
            videoPixelSize.width > 0,
            videoPixelSize.height > 0
        else {
            return nil
        }

        let clampedPoint = CGPoint(
            x: min(max(point.x, bounds.minX), bounds.maxX),
            y: min(max(point.y, bounds.minY), bounds.maxY)
        )
        let normalizedX = clampedPoint.x / bounds.width
        let normalizedY = clampedPoint.y / bounds.height
        let desktopX = max(
            0,
            min(
                Int32(videoPixelSize.width.rounded(.down)) - 1,
                Int32((normalizedX * videoPixelSize.width).rounded(.towardZero))
            )
        )
        let desktopY = max(
            0,
            min(
                Int32(videoPixelSize.height.rounded(.down)) - 1,
                Int32((normalizedY * videoPixelSize.height).rounded(.towardZero))
            )
        )
        return (desktopX, desktopY)
    }

    private func consumeWheelDelta(_ accumulator: inout CGFloat) -> Int {
        let step: CGFloat = 120
        guard abs(accumulator) >= step else {
            return 0
        }

        let direction: CGFloat = accumulator.sign == .minus ? -1 : 1
        accumulator -= direction * step
        return Int(direction * step)
    }
}
