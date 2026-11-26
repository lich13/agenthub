import CoreGraphics
import Foundation

enum SVGPathParser {
    static func parse(_ d: String) -> CGPath {
        let path = CGMutablePath()
        var index = d.startIndex
        let end = d.endIndex

        var current = CGPoint.zero
        var subpathStart = CGPoint.zero
        var lastControl: CGPoint?
        var lastCommand: Character = " "

        func skipSeparators() {
            while index < end {
                let c = d[index]
                if c == " " || c == "," || c == "\t" || c == "\n" || c == "\r" {
                    index = d.index(after: index)
                } else {
                    break
                }
            }
        }

        func hasMoreNumbers() -> Bool {
            skipSeparators()
            guard index < end else { return false }
            let c = d[index]
            return c.isNumber || c == "-" || c == "+" || c == "."
        }

        func readNumber() -> CGFloat? {
            skipSeparators()
            guard index < end else { return nil }
            let start = index
            var seenDot = false
            var seenExp = false

            if d[index] == "+" || d[index] == "-" {
                index = d.index(after: index)
            }
            var hasDigits = false
            while index < end {
                let c = d[index]
                if c.isNumber {
                    hasDigits = true
                    index = d.index(after: index)
                } else if c == "." && !seenDot && !seenExp {
                    seenDot = true
                    index = d.index(after: index)
                } else if (c == "e" || c == "E") && hasDigits && !seenExp {
                    seenExp = true
                    index = d.index(after: index)
                    if index < end && (d[index] == "+" || d[index] == "-") {
                        index = d.index(after: index)
                    }
                } else {
                    break
                }
            }
            guard hasDigits, let val = Double(d[start..<index]) else { return nil }
            return CGFloat(val)
        }

        func readFlag() -> Bool? {
            skipSeparators()
            guard index < end else { return nil }
            let c = d[index]
            if c == "0" {
                index = d.index(after: index)
                return false
            } else if c == "1" {
                index = d.index(after: index)
                return true
            }
            return readNumber().map { $0 != 0 }
        }

        while index < end {
            skipSeparators()
            guard index < end else { break }
            let ch = d[index]
            let cmd: Character
            if ch.isLetter {
                cmd = ch
                index = d.index(after: index)
            } else {
                cmd = lastCommand
            }

            switch cmd {
            case "M", "m":
                let rel = (cmd == "m")
                var firstPair = true
                while hasMoreNumbers(), let x = readNumber(), let y = readNumber() {
                    let pt = rel ? CGPoint(x: current.x + x, y: current.y + y) : CGPoint(x: x, y: y)
                    if firstPair {
                        path.move(to: pt)
                        subpathStart = pt
                        firstPair = false
                    } else {
                        path.addLine(to: pt)
                    }
                    current = pt
                    lastControl = nil
                }
                lastCommand = rel ? "l" : "L"

            case "L", "l":
                let rel = (cmd == "l")
                while hasMoreNumbers(), let x = readNumber(), let y = readNumber() {
                    let pt = rel ? CGPoint(x: current.x + x, y: current.y + y) : CGPoint(x: x, y: y)
                    path.addLine(to: pt)
                    current = pt
                    lastControl = nil
                }
                lastCommand = cmd

            case "H", "h":
                let rel = (cmd == "h")
                while hasMoreNumbers(), let x = readNumber() {
                    let pt = CGPoint(x: rel ? current.x + x : x, y: current.y)
                    path.addLine(to: pt)
                    current = pt
                    lastControl = nil
                }
                lastCommand = cmd

            case "V", "v":
                let rel = (cmd == "v")
                while hasMoreNumbers(), let y = readNumber() {
                    let pt = CGPoint(x: current.x, y: rel ? current.y + y : y)
                    path.addLine(to: pt)
                    current = pt
                    lastControl = nil
                }
                lastCommand = cmd

            case "C", "c":
                let rel = (cmd == "c")
                while hasMoreNumbers(),
                      let x1 = readNumber(), let y1 = readNumber(),
                      let x2 = readNumber(), let y2 = readNumber(),
                      let x = readNumber(), let y = readNumber()
                {
                    let c1 = rel ? CGPoint(x: current.x + x1, y: current.y + y1) : CGPoint(x: x1, y: y1)
                    let c2 = rel ? CGPoint(x: current.x + x2, y: current.y + y2) : CGPoint(x: x2, y: y2)
                    let pt = rel ? CGPoint(x: current.x + x, y: current.y + y) : CGPoint(x: x, y: y)
                    path.addCurve(to: pt, control1: c1, control2: c2)
                    lastControl = c2
                    current = pt
                }
                lastCommand = cmd

            case "S", "s":
                let rel = (cmd == "s")
                while hasMoreNumbers(),
                      let x2 = readNumber(), let y2 = readNumber(),
                      let x = readNumber(), let y = readNumber()
                {
                    let c1: CGPoint
                    if ("CcSs".contains(lastCommand)), let prev = lastControl {
                        c1 = CGPoint(x: 2 * current.x - prev.x, y: 2 * current.y - prev.y)
                    } else {
                        c1 = current
                    }
                    let c2 = rel ? CGPoint(x: current.x + x2, y: current.y + y2) : CGPoint(x: x2, y: y2)
                    let pt = rel ? CGPoint(x: current.x + x, y: current.y + y) : CGPoint(x: x, y: y)
                    path.addCurve(to: pt, control1: c1, control2: c2)
                    lastControl = c2
                    current = pt
                }
                lastCommand = cmd

            case "Q", "q":
                let rel = (cmd == "q")
                while hasMoreNumbers(),
                      let x1 = readNumber(), let y1 = readNumber(),
                      let x = readNumber(), let y = readNumber()
                {
                    let c1 = rel ? CGPoint(x: current.x + x1, y: current.y + y1) : CGPoint(x: x1, y: y1)
                    let pt = rel ? CGPoint(x: current.x + x, y: current.y + y) : CGPoint(x: x, y: y)
                    path.addQuadCurve(to: pt, control: c1)
                    lastControl = c1
                    current = pt
                }
                lastCommand = cmd

            case "T", "t":
                let rel = (cmd == "t")
                while hasMoreNumbers(), let x = readNumber(), let y = readNumber() {
                    let c1: CGPoint
                    if ("QqTt".contains(lastCommand)), let prev = lastControl {
                        c1 = CGPoint(x: 2 * current.x - prev.x, y: 2 * current.y - prev.y)
                    } else {
                        c1 = current
                    }
                    let pt = rel ? CGPoint(x: current.x + x, y: current.y + y) : CGPoint(x: x, y: y)
                    path.addQuadCurve(to: pt, control: c1)
                    lastControl = c1
                    current = pt
                }
                lastCommand = cmd

            case "A", "a":
                let rel = (cmd == "a")
                while hasMoreNumbers(),
                      let rx = readNumber(), let ry = readNumber(),
                      let angle = readNumber(),
                      let largeArc = readFlag(),
                      let sweep = readFlag(),
                      let x = readNumber(), let y = readNumber()
                {
                    let pt = rel ? CGPoint(x: current.x + x, y: current.y + y) : CGPoint(x: x, y: y)
                    addSvgArc(
                        to: path,
                        from: current,
                        end: pt,
                        rx: abs(rx),
                        ry: abs(ry),
                        xAxisRotationDegrees: angle,
                        largeArcFlag: largeArc,
                        sweepFlag: sweep
                    )
                    current = pt
                    lastControl = nil
                }
                lastCommand = cmd

            case "Z", "z":
                path.closeSubpath()
                current = subpathStart
                lastControl = nil
                lastCommand = cmd

            default:
                index = d.index(after: index)
            }
        }
        return path
    }

    private static func addSvgArc(
        to path: CGMutablePath,
        from p1: CGPoint,
        end p2: CGPoint,
        rx rawRx: CGFloat,
        ry rawRy: CGFloat,
        xAxisRotationDegrees: CGFloat,
        largeArcFlag: Bool,
        sweepFlag: Bool
    ) {
        if p1 == p2 { return }
        if rawRx == 0 || rawRy == 0 {
            path.addLine(to: p2)
            return
        }
        var rx = rawRx
        var ry = rawRy
        let phi = xAxisRotationDegrees * .pi / 180.0
        let cosPhi = cos(phi)
        let sinPhi = sin(phi)

        let dx2 = (p1.x - p2.x) / 2.0
        let dy2 = (p1.y - p2.y) / 2.0
        let x1p = cosPhi * dx2 + sinPhi * dy2
        let y1p = -sinPhi * dx2 + cosPhi * dy2

        var rxSq = rx * rx
        var rySq = ry * ry
        let x1pSq = x1p * x1p
        let y1pSq = y1p * y1p

        let radiiCheck = x1pSq / rxSq + y1pSq / rySq
        if radiiCheck > 1.0 {
            let scale = sqrt(radiiCheck)
            rx *= scale
            ry *= scale
            rxSq = rx * rx
            rySq = ry * ry
        }

        let sign: CGFloat = (largeArcFlag == sweepFlag) ? -1.0 : 1.0
        let num = max(0.0, rxSq * rySq - rxSq * y1pSq - rySq * x1pSq)
        let den = rxSq * y1pSq + rySq * x1pSq
        let coef = sign * (den > 0 ? sqrt(num / den) : 0)
        let cxp = coef * ((rx * y1p) / ry)
        let cyp = coef * -((ry * x1p) / rx)

        let cx = cosPhi * cxp - sinPhi * cyp + (p1.x + p2.x) / 2.0
        let cy = sinPhi * cxp + cosPhi * cyp + (p1.y + p2.y) / 2.0

        func vectorAngle(_ ux: CGFloat, _ uy: CGFloat, _ vx: CGFloat, _ vy: CGFloat) -> CGFloat {
            let dot = ux * vx + uy * vy
            let len = sqrt(ux * ux + uy * uy) * sqrt(vx * vx + vy * vy)
            guard len > 0 else { return 0 }
            let clamped = max(-1.0, min(1.0, dot / len))
            let angle = acos(clamped)
            return (ux * vy - uy * vx < 0) ? -angle : angle
        }

        let ux = (x1p - cxp) / rx
        let uy = (y1p - cyp) / ry
        let vx = (-x1p - cxp) / rx
        let vy = (-y1p - cyp) / ry

        let theta1 = vectorAngle(1, 0, ux, uy)
        var deltaTheta = vectorAngle(ux, uy, vx, vy)
        if !sweepFlag && deltaTheta > 0 {
            deltaTheta -= 2.0 * .pi
        } else if sweepFlag && deltaTheta < 0 {
            deltaTheta += 2.0 * .pi
        }

        let segments = max(1, Int(ceil(abs(deltaTheta) / (.pi / 2.0))))
        let delta = deltaTheta / CGFloat(segments)
        let alpha = (4.0 / 3.0) * tan(delta / 4.0)

        for i in 0..<segments {
            let t1 = theta1 + CGFloat(i) * delta
            let t2 = t1 + delta
            let cosT1 = cos(t1), sinT1 = sin(t1)
            let cosT2 = cos(t2), sinT2 = sin(t2)

            let e1x = cosT1 - alpha * sinT1
            let e1y = sinT1 + alpha * cosT1
            let e2x = cosT2 + alpha * sinT2
            let e2y = sinT2 - alpha * cosT2

            func mapPoint(_ ex: CGFloat, _ ey: CGFloat) -> CGPoint {
                let sx = ex * rx
                let sy = ey * ry
                return CGPoint(
                    x: cosPhi * sx - sinPhi * sy + cx,
                    y: sinPhi * sx + cosPhi * sy + cy
                )
            }

            let c1 = mapPoint(e1x, e1y)
            let c2 = mapPoint(e2x, e2y)
            let endPt = mapPoint(cosT2, sinT2)
            path.addCurve(to: endPt, control1: c1, control2: c2)
        }
    }
}
