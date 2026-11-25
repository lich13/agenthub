import AppKit
import SwiftUI

@MainActor
final class FlippedDocumentView: NSView {
    override var isFlipped: Bool { true }
}

extension ConversationRowCellView {
    func buildMarkdownViews(
        ownerId: String,
        markdown: String,
        foldInstructions: Bool,
        width: CGFloat,
        coordinator: ConversationTableCoordinator,
        into targetStack: NSStackView? = nil
    ) {
        let destStack = targetStack ?? containerStack
        let palette = coordinator.palette
        let segs = GfmLayoutCache.shared.getOrRender(
            markdown: markdown,
            foldInstructions: foldInstructions,
            scheme: coordinator.scheme,
            workspace: coordinator.workspace,
            transport: coordinator.store.transport
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
        let tv = ReaderSelectableTextView(frame: NSRect(x: 0, y: 0, width: width, height: 24))
        tv.quoteBackgroundColor = palette.surface.nsColor
        tv.quoteAccentColor = palette.accent.nsColor
        tv.applyAttributedText(attr, width: width)
        let h = max(22, tv.measuredUsedHeight())
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
        let normalizedCode = ConversationTableCoordinator.normalizeCodeBlockText(code)
        let box = NSView()
        box.translatesAutoresizingMaskIntoConstraints = false
        box.wantsLayer = true
        box.layer?.cornerRadius = 12
        box.layer?.backgroundColor = palette.codeBg.nsColor.cgColor
        box.layer?.borderWidth = 1
        box.layer?.borderColor = palette.borderSubtle.nsColor.cgColor

        let copyBtn = IconFeedbackActionButton(
            dimension: 32,
            iconSize: 15,
            idleIcon: .copy,
            palette: palette,
            borderedSurface: palette.elevated.nsColor,
            borderColor: palette.borderStrong.nsColor,
            toolTipText: "复制代码"
        ) { [weak coordinator] in
            NSPasteboard.general.clearContents()
            NSPasteboard.general.setString(normalizedCode, forType: .string)
            coordinator?.onCopyFeedback?("已复制代码")
        }

        let innerCodeW = max(32, width - 32)
        let codeHeight = ConversationTableCoordinator.measureCodeBlockHeight(code: normalizedCode)
        let docWidth = ConversationTableCoordinator.measureCodeDocumentWidth(code: normalizedCode, minWidth: innerCodeW)
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
        scroll.documentView = codeTv

        box.addSubview(scroll)
        box.addSubview(copyBtn)

        NSLayoutConstraint.activate([
            box.widthAnchor.constraint(equalToConstant: width),
            copyBtn.trailingAnchor.constraint(equalTo: box.trailingAnchor, constant: -8),
            copyBtn.topAnchor.constraint(equalTo: box.topAnchor, constant: 8),
            copyBtn.widthAnchor.constraint(equalToConstant: 32),
            copyBtn.heightAnchor.constraint(equalToConstant: 32),
            scroll.leadingAnchor.constraint(equalTo: box.leadingAnchor, constant: 16),
            scroll.trailingAnchor.constraint(equalTo: box.trailingAnchor, constant: -16),
            scroll.topAnchor.constraint(equalTo: box.topAnchor, constant: 38),
            scroll.bottomAnchor.constraint(equalTo: box.bottomAnchor, constant: -16),
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
        let layout = ConversationTableCoordinator.measureTableLayout(headers: headers, rows: rows, maxWidth: width)
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
        let docView = FlippedDocumentView(frame: NSRect(x: 0, y: 0, width: layout.totalWidth, height: layout.totalHeight))
        var currentTopY: CGFloat = 0

        for (rIdx, cols) in allRows.enumerated() {
            let rowH = layout.rowHeights[rIdx]
            var currentX: CGFloat = 0
            for cIdx in 0 ..< colCount {
                let colW = layout.colWidths[cIdx]
                let cellRect = NSRect(x: currentX, y: currentTopY, width: colW, height: rowH)
                let cellBox = FlippedDocumentView(frame: cellRect)
                cellBox.wantsLayer = true
                cellBox.layer?.backgroundColor = (rIdx == 0 ? palette.surface : palette.background).nsColor.cgColor
                cellBox.layer?.borderWidth = 0.5
                cellBox.layer?.borderColor = palette.borderStrong.nsColor.cgColor

                let cellAttr = cIdx < cols.count ? cols[cIdx] : NSAttributedString(string: "")
                let innerW = max(36, colW - 24)
                let textH = max(18, ReaderSelectableTextView.measureHeight(attributed: cellAttr, width: innerW))
                let tv = ReaderSelectableTextView(
                    frame: NSRect(x: 12, y: max(4, floor((rowH - textH) * 0.5)), width: innerW, height: textH)
                )
                tv.quoteBackgroundColor = palette.surface.nsColor
                tv.quoteAccentColor = palette.accent.nsColor
                tv.applyAttributedText(cellAttr, width: innerW)
                tv.onLinkActivated = { [weak self, weak coordinator] href in
                    guard let self, let coordinator else { return false }
                    return self.handleActivatedHref(href, coordinator: coordinator)
                }
                cellBox.addSubview(tv)
                docView.addSubview(cellBox)
                currentX += colW
            }
            currentTopY += rowH
        }
        docView.wantsLayer = true
        docView.layer?.borderWidth = 1
        docView.layer?.borderColor = palette.borderStrong.nsColor.cgColor
        scroll.documentView = docView
        NSLayoutConstraint.activate([
            scroll.widthAnchor.constraint(equalToConstant: visibleW),
            scroll.heightAnchor.constraint(equalToConstant: layout.totalHeight + scrollerExtra),
        ])
        return scroll
    }
}
