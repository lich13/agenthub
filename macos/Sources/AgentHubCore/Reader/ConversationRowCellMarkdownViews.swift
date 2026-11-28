import AppKit
import SwiftUI

@MainActor
class FlippedDocumentView: NSView {
    override var isFlipped: Bool { true }
}

@MainActor
final class TableGridDocumentView: FlippedDocumentView {
    let colWidths: [CGFloat]
    let rowHeights: [CGFloat]
    let headerBackground: NSColor
    let bodyBackground: NSColor
    let borderColor: NSColor

    init(
        frame: NSRect,
        colWidths: [CGFloat],
        rowHeights: [CGFloat],
        headerBackground: NSColor,
        bodyBackground: NSColor,
        borderColor: NSColor
    ) {
        self.colWidths = colWidths
        self.rowHeights = rowHeights
        self.headerBackground = headerBackground
        self.bodyBackground = bodyBackground
        self.borderColor = borderColor
        super.init(frame: frame)
    }

    override var isOpaque: Bool { true }

    @available(*, unavailable)
    required init?(coder: NSCoder) {
        fatalError("init(coder:) has not been implemented")
    }

    override func draw(_ dirtyRect: NSRect) {
        let b = bounds
        guard b.width > 1, b.height > 1 else { return }

        bodyBackground.setFill()
        b.intersection(dirtyRect).fill()

        if let firstRowH = rowHeights.first, firstRowH > 0 {
            let headerRect = NSRect(x: 0, y: 0, width: b.width, height: min(b.height, firstRowH))
            let clipped = headerRect.intersection(dirtyRect)
            if !clipped.isNull && !clipped.isEmpty {
                headerBackground.setFill()
                clipped.fill()
            }
        }

        borderColor.setFill()
        var x: CGFloat = 0
        for idx in 0 ..< max(0, colWidths.count - 1) {
            x += colWidths[idx]
            let vLine = NSRect(x: x - 0.5, y: 0, width: 1.0, height: b.height).intersection(dirtyRect)
            if !vLine.isNull && !vLine.isEmpty {
                vLine.fill()
            }
        }

        var y: CGFloat = 0
        for idx in 0 ..< max(0, rowHeights.count - 1) {
            y += rowHeights[idx]
            let hLine = NSRect(x: 0, y: y - 0.5, width: b.width, height: 1.0).intersection(dirtyRect)
            if !hLine.isNull && !hLine.isEmpty {
                hLine.fill()
            }
        }

        borderColor.setStroke()
        let outerPath = NSBezierPath(rect: b.insetBy(dx: 0.5, dy: 0.5))
        outerPath.lineWidth = 1.0
        outerPath.stroke()
    }
}

@MainActor
private final class CodeBlockContainerView: NSView {
    let copyButton: IconFeedbackActionButton
    private var tracking: NSTrackingArea?
    private var isHovered = false {
        didSet { updateCopyVisibility() }
    }

    init(copyButton: IconFeedbackActionButton) {
        self.copyButton = copyButton
        super.init(frame: .zero)
        translatesAutoresizingMaskIntoConstraints = false
        wantsLayer = true
    }

    @available(*, unavailable)
    required init?(coder: NSCoder) {
        fatalError("init(coder:) has not been implemented")
    }

    override func updateTrackingAreas() {
        super.updateTrackingAreas()
        if let tracking { removeTrackingArea(tracking) }
        let area = NSTrackingArea(
            rect: bounds,
            options: [.mouseEnteredAndExited, .activeInActiveApp, .inVisibleRect],
            owner: self,
            userInfo: nil
        )
        addTrackingArea(area)
        tracking = area
    }

    override func layout() {
        super.layout()
        updateCopyVisibility()
    }

    override func mouseEntered(with event: NSEvent) {
        isHovered = true
    }

    override func mouseExited(with event: NSEvent) {
        isHovered = false
    }

    func updateCopyVisibility() {
        let narrow = bounds.width > 0 && bounds.width <= LayoutMetrics.narrowBreakpoint
        let focusedInside = (window?.firstResponder as? NSView).map { $0 === self || $0.isDescendant(of: self) } ?? false
        let visible = isHovered || focusedInside || copyButton.isShowingCopied || narrow
        copyButton.alphaValue = visible ? 1.0 : 0.0
    }
}

extension ConversationRowCellView {
    func buildMarkdownViews(
        ownerId: String,
        markdown: String,
        foldInstructions: Bool,
        width: CGFloat,
        coordinator: ConversationTableCoordinator,
        into targetStack: NSStackView? = nil,
        alreadyCleaned: Bool = false
    ) {
        let destStack = targetStack ?? containerStack
        let palette = coordinator.palette
        let segs = GfmLayoutCache.shared.getOrRender(
            markdown: markdown,
            foldInstructions: foldInstructions,
            scheme: coordinator.scheme,
            workspace: coordinator.workspace,
            transport: coordinator.store.transport,
            alreadyCleaned: alreadyCleaned
        )

        for seg in segs {
            switch seg {
            case let .prose(attr):
                let tv = makeProseTextView(attr: attr, width: width, coordinator: coordinator)
                destStack.addArrangedSubview(tv)

            case let .codeBlock(lang, code):
                destStack.addArrangedSubview(makeCodeBlockView(lang: lang, code: code, width: width, coordinator: coordinator))

            case let .table(_, headers, rows):
                destStack.addArrangedSubview(makeTableView(headers: headers, rows: rows, width: width, palette: palette, coordinator: coordinator))

            case .thematicBreak:
                let rule = NSView()
                rule.wantsLayer = true
                rule.layer?.backgroundColor = palette.borderSubtle.nsColor.cgColor
                rule.translatesAutoresizingMaskIntoConstraints = false
                NSLayoutConstraint.activate([
                    rule.widthAnchor.constraint(equalToConstant: width),
                    rule.heightAnchor.constraint(equalToConstant: 1),
                ])
                destStack.addArrangedSubview(rule)

            case let .instructionsFold(id, text, lines, bytes):
                let scopedKey = "\(ownerId):\(id)"
                let open = coordinator.disclosureStore.isOpen(scope: coordinator.sessionScopeKey, stateKey: scopedKey, initiallyOpen: false)
                destStack.addArrangedSubview(
                    makeInstructionFileCard(
                        scopedKey: scopedKey,
                        title: "AGENTS.md",
                        meta: "\(lines) 行 · \(bytes) 字节",
                        markdown: text,
                        open: open,
                        width: width,
                        palette: palette,
                        coordinator: coordinator
                    )
                )
            }
        }
    }

    private func makeInstructionFileCard(
        scopedKey: String,
        title: String,
        meta: String,
        markdown: String,
        open: Bool,
        width: CGFloat,
        palette: DesignPalette,
        coordinator: ConversationTableCoordinator
    ) -> NSView {
        let card = NSView()
        card.translatesAutoresizingMaskIntoConstraints = false
        card.wantsLayer = true
        card.layer?.cornerRadius = 10
        card.layer?.backgroundColor = palette.surface.nsColor.cgColor
        card.layer?.borderWidth = 1
        card.layer?.borderColor = palette.borderSubtle.nsColor.cgColor

        let header = ActionRowContainerView()
        header.hoverFill = palette.hover.nsColor
        header.focusRingColor = palette.accent.nsColor
        header.accessibilityExpandedState = open
        header.setAccessibilityLabel(title)
        header.translatesAutoresizingMaskIntoConstraints = false
        header.onTap = { [weak coordinator] in
            coordinator?.toggleDisclosurePreservingAnchor(stateKey: scopedKey, currentlyOpen: open)
        }
        let hStack = NSStackView()
        hStack.orientation = .horizontal
        hStack.alignment = .centerY
        hStack.spacing = 8
        hStack.translatesAutoresizingMaskIntoConstraints = false
        header.addSubview(hStack)

        let titleLbl = NSTextField(labelWithString: title)
        titleLbl.font = DesignFonts.nsBodyFont(size: 14)
        titleLbl.textColor = palette.textPrimary.nsColor

        let metaLbl = NSTextField(labelWithString: meta)
        metaLbl.font = DesignFonts.nsBodyFont(size: 12)
        metaLbl.textColor = palette.textMuted.nsColor

        let chev = NSImageView(
            image: LucideVectorCatalog.nsImage(
                for: open ? .chevronDown : .chevronRight,
                size: 16,
                color: (open ? palette.accent : palette.textMuted).nsColor
            )
        )
        chev.translatesAutoresizingMaskIntoConstraints = false
        NSLayoutConstraint.activate([
            card.widthAnchor.constraint(equalToConstant: width),
            chev.widthAnchor.constraint(equalToConstant: 16),
            chev.heightAnchor.constraint(equalToConstant: 16),
            header.heightAnchor.constraint(equalToConstant: 30),
            hStack.leadingAnchor.constraint(equalTo: header.leadingAnchor),
            hStack.trailingAnchor.constraint(equalTo: header.trailingAnchor),
            hStack.centerYAnchor.constraint(equalTo: header.centerYAnchor),
        ])
        hStack.addArrangedSubview(titleLbl)
        hStack.addArrangedSubview(NSView())
        hStack.addArrangedSubview(metaLbl)
        hStack.addArrangedSubview(chev)
        card.addSubview(header)

        if !open {
            NSLayoutConstraint.activate([
                header.leadingAnchor.constraint(equalTo: card.leadingAnchor, constant: 12),
                header.trailingAnchor.constraint(equalTo: card.trailingAnchor, constant: -12),
                header.topAnchor.constraint(equalTo: card.topAnchor, constant: 4),
                header.bottomAnchor.constraint(equalTo: card.bottomAnchor, constant: -4),
            ])
            return card
        }

        let innerStack = NSStackView()
        innerStack.orientation = .vertical
        innerStack.alignment = .leading
        innerStack.spacing = 8
        innerStack.translatesAutoresizingMaskIntoConstraints = false
        card.addSubview(innerStack)
        buildMarkdownViews(
            ownerId: scopedKey,
            markdown: markdown,
            foldInstructions: false,
            width: max(120, width - 24),
            coordinator: coordinator,
            into: innerStack
        )
        NSLayoutConstraint.activate([
            header.leadingAnchor.constraint(equalTo: card.leadingAnchor, constant: 12),
            header.trailingAnchor.constraint(equalTo: card.trailingAnchor, constant: -12),
            header.topAnchor.constraint(equalTo: card.topAnchor, constant: 4),
            innerStack.leadingAnchor.constraint(equalTo: card.leadingAnchor, constant: 12),
            innerStack.trailingAnchor.constraint(equalTo: card.trailingAnchor, constant: -12),
            innerStack.topAnchor.constraint(equalTo: header.bottomAnchor, constant: 10),
            innerStack.bottomAnchor.constraint(equalTo: card.bottomAnchor, constant: -10),
        ])
        return card
    }

    func handleActivatedHref(_ href: String, coordinator: ConversationTableCoordinator) -> Bool {
        if href.hasPrefix("agenthub-copy-file:") {
            let rawFileHref = String(href.dropFirst("agenthub-copy-file:".count))
            let target = FileLinksAndPlanExport.fileLinkTarget(rawFileHref, cwd: coordinator.cwd)
            let copyPath = target?.path ?? rawFileHref
            NSPasteboard.general.clearContents()
            NSPasteboard.general.setString(copyPath, forType: .string)
            coordinator.onCopyFeedback?(
                (target?.resolved == false) ? "已复制原路径；缺少工作目录，无法定位。" : "已复制路径"
            )
            return true
        }
        if let fileTarget = FileLinksAndPlanExport.fileLinkTarget(href, cwd: coordinator.cwd) {
            if coordinator.store.target == .local && fileTarget.resolved {
                let targetPath = fileTarget.path
                Task { @MainActor [weak coordinator] in
                    do {
                        try await FileOpenerService.revealPaths([targetPath])
                    } catch {
                        coordinator?.onCopyFeedback?("无法在 Finder 中显示，请检查文件是否仍存在")
                    }
                }
            } else {
                NSPasteboard.general.clearContents()
                NSPasteboard.general.setString(fileTarget.path, forType: .string)
                coordinator.onCopyFeedback?(
                    fileTarget.resolved ? "已复制路径" : "已复制原路径；缺少工作目录，无法定位。"
                )
            }
            return true
        }
        if let url = URL(string: href),
           let scheme = url.scheme?.lowercased(),
           scheme == "https" || scheme == "http" || scheme == "mailto"
        {
            NSWorkspace.shared.open(url)
            return true
        }
        return false
    }

    func makeProseTextView(
        attr: NSAttributedString,
        width: CGFloat,
        coordinator: ConversationTableCoordinator
    ) -> ReaderSelectableTextView {
        let palette = coordinator.palette
        let h = coordinator.cachedProseHeight(attributed: attr, width: width)
        let tv = ReaderSelectableTextView(frame: NSRect(x: 0, y: 0, width: width, height: h))
        tv.quoteBackgroundColor = palette.surface.nsColor
        tv.quoteAccentColor = palette.accent.nsColor
        tv.applyAttributedText(attr, width: width)
        tv.setFrameSize(NSSize(width: width, height: h))
        tv.translatesAutoresizingMaskIntoConstraints = false
        NSLayoutConstraint.activate([
            tv.widthAnchor.constraint(equalToConstant: width),
            tv.heightAnchor.constraint(equalToConstant: h),
        ])
        tv.onLinkActivated = { [weak self, weak coordinator] href in
            guard let self, let coordinator else { return false }
            return self.handleActivatedHref(href, coordinator: coordinator)
        }
        tv.onFocusStateChanged = { [weak self] in
            self?.updateFloatingCopyVisibility()
        }
        return tv
    }

    func makeCodeBlockView(
        lang _: String?,
        code: String,
        width: CGFloat,
        coordinator: ConversationTableCoordinator
    ) -> NSView {
        let palette = coordinator.palette
        let innerCodeW = max(32, width - 24)
        let codeLayout = coordinator.cachedCodeLayout(code: code, innerWidth: innerCodeW)
        let normalizedCode = codeLayout.normalized
        let codeHeight = codeLayout.codeHeight
        let docWidth = codeLayout.docWidth

        let copyBtn = IconFeedbackActionButton(
            dimension: 28,
            iconSize: 15,
            idleIcon: .copy,
            palette: palette,
            borderedSurface: palette.elevated.nsColor.withAlphaComponent(0.86),
            borderColor: palette.borderStrong.nsColor.withAlphaComponent(0.82),
            toolTipText: "复制代码"
        ) { [weak coordinator] in
            NSPasteboard.general.clearContents()
            NSPasteboard.general.setString(normalizedCode, forType: .string)
            coordinator?.onCopyFeedback?("已复制代码")
        }

        let box = CodeBlockContainerView(copyButton: copyBtn)
        box.layer?.cornerRadius = 12
        box.layer?.backgroundColor = palette.codeBg.nsColor.cgColor
        box.layer?.borderWidth = 1
        box.layer?.borderColor = palette.borderSubtle.nsColor.cgColor
        copyBtn.onFeedbackStateChanged = { [weak box] in
            box?.updateCopyVisibility()
        }

        let needsHScroll = docWidth > innerCodeW + 0.5
        let scrollerExtra = ConversationTableCoordinator.horizontalScrollerExtraHeight(needsHorizontalScroll: needsHScroll)

        let scroll = PassthroughInnerScrollView(allowsVerticalScroll: false)
        scroll.hasHorizontalScroller = needsHScroll
        scroll.hasVerticalScroller = false
        scroll.autohidesScrollers = true
        scroll.drawsBackground = false
        scroll.translatesAutoresizingMaskIntoConstraints = false

        let pStyle = NSMutableParagraphStyle()
        pStyle.minimumLineHeight = 20.8 // 13pt * 1.6
        pStyle.maximumLineHeight = 20.8
        let codeAttr = NSAttributedString(
            string: normalizedCode,
            attributes: [
                .font: DesignFonts.nsMonoFont(size: 13),
                .foregroundColor: palette.textPrimary.nsColor,
                .paragraphStyle: pStyle,
            ]
        )

        let codeTv = ReaderSelectableTextView(frame: NSRect(x: 0, y: 0, width: docWidth, height: codeHeight))
        codeTv.isRichText = false
        codeTv.isHorizontallyResizable = true
        codeTv.isVerticallyResizable = false
        codeTv.textContainerInset = NSSize(width: 0, height: 6)
        codeTv.textContainer?.lineFragmentPadding = 0
        codeTv.textContainer?.widthTracksTextView = false
        codeTv.textContainer?.containerSize = NSSize(width: CGFloat.greatestFiniteMagnitude, height: codeHeight)
        codeTv.textStorage?.setAttributedString(codeAttr)
        if let manager = codeTv.layoutManager, let container = codeTv.textContainer {
            manager.ensureLayout(for: container)
        }
        codeTv.onFocusStateChanged = { [weak box] in
            box?.updateCopyVisibility()
        }
        scroll.documentView = codeTv

        box.addSubview(scroll)
        box.addSubview(copyBtn)
        box.updateCopyVisibility()

        NSLayoutConstraint.activate([
            box.widthAnchor.constraint(equalToConstant: width),
            copyBtn.trailingAnchor.constraint(equalTo: box.trailingAnchor, constant: -8),
            copyBtn.topAnchor.constraint(equalTo: box.topAnchor, constant: 8),
            copyBtn.widthAnchor.constraint(equalToConstant: 28),
            copyBtn.heightAnchor.constraint(equalToConstant: 28),
            scroll.leadingAnchor.constraint(equalTo: box.leadingAnchor, constant: 12),
            scroll.trailingAnchor.constraint(equalTo: box.trailingAnchor, constant: -12),
            scroll.topAnchor.constraint(equalTo: box.topAnchor, constant: 12),
            scroll.bottomAnchor.constraint(equalTo: box.bottomAnchor, constant: -12),
            scroll.heightAnchor.constraint(equalToConstant: codeHeight + scrollerExtra),
        ])
        return box
    }

    func makeTableView(
        headers: [NSAttributedString],
        rows: [[NSAttributedString]],
        width: CGFloat,
        palette: DesignPalette,
        coordinator: ConversationTableCoordinator
    ) -> NSView {
        let layout = coordinator.cachedTableLayout(headers: headers, rows: rows, maxWidth: width)
        let visibleW = min(width, layout.totalWidth)
        let needsHScroll = layout.totalWidth > visibleW + 0.5
        let scrollerExtra = ConversationTableCoordinator.horizontalScrollerExtraHeight(needsHorizontalScroll: needsHScroll)

        let scroll = PassthroughInnerScrollView(allowsVerticalScroll: false)
        scroll.hasHorizontalScroller = needsHScroll
        scroll.hasVerticalScroller = false
        scroll.autohidesScrollers = true
        scroll.drawsBackground = false
        scroll.translatesAutoresizingMaskIntoConstraints = false

        let colCount = layout.colWidths.count
        let allRows = [headers] + rows
        let docView = TableGridDocumentView(
            frame: NSRect(x: 0, y: 0, width: layout.totalWidth, height: layout.totalHeight),
            colWidths: layout.colWidths,
            rowHeights: layout.rowHeights,
            headerBackground: palette.surface.nsColor,
            bodyBackground: palette.background.nsColor,
            borderColor: palette.borderStrong.nsColor
        )
        var currentTopY: CGFloat = 0

        for (rIdx, cols) in allRows.enumerated() {
            let rowH = layout.rowHeights[rIdx]
            var currentX: CGFloat = 0
            for cIdx in 0 ..< colCount {
                let colW = layout.colWidths[cIdx]
                let cellAttr = cIdx < cols.count ? cols[cIdx] : NSAttributedString(string: "")
                let innerW = max(36, colW - 24)
                let tv = ReaderSelectableTextView(
                    frame: NSRect(x: currentX + 12, y: currentTopY + 4, width: innerW, height: max(18, rowH - 8))
                )
                tv.quoteBackgroundColor = palette.surface.nsColor
                tv.quoteAccentColor = palette.accent.nsColor
                tv.applyAttributedText(cellAttr, width: innerW)
                let textH = max(18, tv.measuredUsedHeight())
                tv.frame = NSRect(
                    x: currentX + 12,
                    y: currentTopY + max(4, floor((rowH - textH) * 0.5)),
                    width: innerW,
                    height: textH
                )
                tv.onLinkActivated = { [weak self, weak coordinator] href in
                    guard let self, let coordinator else { return false }
                    return self.handleActivatedHref(href, coordinator: coordinator)
                }
                docView.addSubview(tv)
                currentX += colW
            }
            currentTopY += rowH
        }
        scroll.documentView = docView
        NSLayoutConstraint.activate([
            scroll.widthAnchor.constraint(equalToConstant: visibleW),
            scroll.heightAnchor.constraint(equalToConstant: layout.totalHeight + scrollerExtra),
        ])
        return scroll
    }
}
