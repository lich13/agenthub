import AppKit
import SwiftUI

// MARK: - AppKit NSTableView Coordinator with Deterministic 0.0pt Scroll Anchor Lock

@MainActor
public final class ConversationRowCellView: NSTableCellView {
    private let containerStack = NSStackView()
    public private(set) var floatingSideCopyButton: IconFeedbackActionButton?
    public private(set) var questionReplyCopyButtons: [IconFeedbackActionButton] = []
    private var hoverTrackingArea: NSTrackingArea?
    public private(set) var isRowHovered: Bool = false {
        didSet { updateFloatingCopyVisibility() }
    }
    private var configuredLeadingInset: CGFloat = 0
    private var configuredContentWidth: CGFloat = 0
    private var stackConstraints: [NSLayoutConstraint] = []

    private struct CellConfigSignature: Equatable {
        let sessionScopeKey: String
        let item: ConversationRowItem
        let leadingInset: CGFloat
        let contentWidth: CGFloat
        let highlighted: Bool
        let scheme: ColorSchemeKind
        let workspace: WorkspaceId
        let disclosureRev: UInt64
    }

    private var lastConfiguredSignature: CellConfigSignature?

    public init(identifier: NSUserInterfaceItemIdentifier) {
        super.init(frame: .zero)
        self.identifier = identifier
        clipsToBounds = false
        wantsLayer = true
        containerStack.clipsToBounds = false
        containerStack.orientation = .vertical
        containerStack.alignment = .leading
        containerStack.spacing = 8
        containerStack.translatesAutoresizingMaskIntoConstraints = false
        addSubview(containerStack)
    }

    @available(*, unavailable)
    required init?(coder: NSCoder) {
        fatalError("init(coder:) has not been implemented")
    }

    /// Computes X origin for the 30x30pt floating reply copy button:
    /// `right: -36pt` relative to the `768pt` content column (`leadingInset + contentWidth + 6`),
    /// clamped inside viewport (`cellWidth - 34`) on narrow widths.
    public static func floatingSideCopyButtonX(
        leadingInset: CGFloat,
        contentWidth: CGFloat,
        cellWidth: CGFloat
    ) -> CGFloat {
        let desired = leadingInset + contentWidth + 6
        return max(8, min(max(8, cellWidth - 34), desired))
    }

    override public func updateTrackingAreas() {
        super.updateTrackingAreas()
        if let hoverTrackingArea {
            removeTrackingArea(hoverTrackingArea)
        }
        let area = NSTrackingArea(
            rect: bounds,
            options: [.mouseEnteredAndExited, .activeInActiveApp, .inVisibleRect],
            owner: self,
            userInfo: nil
        )
        addTrackingArea(area)
        hoverTrackingArea = area
    }

    override public func mouseEntered(with event: NSEvent) {
        isRowHovered = true
    }

    override public func mouseExited(with event: NSEvent) {
        isRowHovered = false
    }

    override public func layout() {
        super.layout()
        positionFloatingCopyButton()
    }

    private func positionFloatingCopyButton() {
        guard let btn = floatingSideCopyButton else { return }
        let totalW = bounds.width > 0 ? bounds.width : (configuredLeadingInset + configuredContentWidth + 46)
        let x = Self.floatingSideCopyButtonX(
            leadingInset: configuredLeadingInset,
            contentWidth: configuredContentWidth,
            cellWidth: totalW
        )
        // NSView is non-flipped (y=0 at bottom); top: -2pt relative to containerStack (topAnchor + 2) => top offset 0pt!
        let y = max(0, bounds.height - 30)
        btn.frame = NSRect(x: x, y: y, width: 30, height: 30)
        updateFloatingCopyVisibility()
    }

    public func setSimulatedHoverForTesting(_ hovered: Bool) {
        isRowHovered = hovered
    }

    private func updateFloatingCopyVisibility() {
        let narrowColumn = bounds.width > 0 && bounds.width <= LayoutMetrics.narrowBreakpoint
        let firstRespView = window?.firstResponder as? NSView
        let focusedInside = firstRespView.map { $0 === self || $0.isDescendant(of: self) } ?? false

        if let btn = floatingSideCopyButton {
            let visible = isRowHovered || btn.isShowingCopied || focusedInside || narrowColumn
            btn.alphaValue = visible ? 1.0 : 0.0
        }
        for qBtn in questionReplyCopyButtons {
            let visible = isRowHovered || qBtn.isShowingCopied || focusedInside || narrowColumn
            qBtn.alphaValue = visible ? 1.0 : 0.0
        }
    }

    public func configure(
        item: ConversationRowItem,
        leadingInset: CGFloat,
        trailingInset: CGFloat,
        contentWidth: CGFloat,
        highlighted: Bool,
        coordinator: ConversationTableCoordinator
    ) {
        let nextSig = CellConfigSignature(
            sessionScopeKey: coordinator.sessionScopeKey,
            item: item,
            leadingInset: leadingInset,
            contentWidth: contentWidth,
            highlighted: highlighted,
            scheme: coordinator.scheme,
            workspace: coordinator.workspace,
            disclosureRev: coordinator.disclosureStore.revision
        )
        if lastConfiguredSignature == nextSig, !containerStack.arrangedSubviews.isEmpty {
            positionFloatingCopyButton()
            return
        }
        lastConfiguredSignature = nextSig

        configuredLeadingInset = leadingInset
        configuredContentWidth = contentWidth
        floatingSideCopyButton?.removeFromSuperview()
        floatingSideCopyButton = nil
        questionReplyCopyButtons.removeAll()
        isRowHovered = false

        containerStack.arrangedSubviews.forEach { v in
            containerStack.removeArrangedSubview(v)
            v.removeFromSuperview()
        }
        NSLayoutConstraint.deactivate(stackConstraints)
        stackConstraints = [
            containerStack.leadingAnchor.constraint(equalTo: leadingAnchor, constant: leadingInset),
            containerStack.topAnchor.constraint(equalTo: topAnchor, constant: 2),
        ]
        NSLayoutConstraint.activate(stackConstraints)

        let palette = coordinator.palette
        layer?.cornerRadius = 10
        layer?.backgroundColor = highlighted
            ? palette.accentBorder.nsColor.withAlphaComponent(0.28).cgColor
            : NSColor.clear.cgColor

        switch item.kind {
        case let .user(msg, rawText, namesOnly):
            buildUserView(item: item, msg: msg, rawText: rawText, namesOnly: namesOnly, width: contentWidth, coordinator: coordinator)
        case let .assistant(markdown, foldInstructions):
            buildMarkdownViews(ownerId: item.id, markdown: markdown, foldInstructions: foldInstructions, width: contentWidth, coordinator: coordinator)
            let cleanedReply = VisibleMarkdownEngine.visibleMarkdown(markdown)
            if !cleanedReply.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty {
                let floatBtn = IconFeedbackActionButton(
                    dimension: 30,
                    iconSize: 17,
                    idleIcon: .copy,
                    palette: palette,
                    toolTipText: "复制答复"
                ) { [weak self, weak coordinator] in
                    NSPasteboard.general.clearContents()
                    NSPasteboard.general.setString(cleanedReply, forType: .string)
                    coordinator?.onCopyFeedback?("已复制答复")
                    self?.updateFloatingCopyVisibility()
                }
                floatBtn.onFeedbackStateChanged = { [weak self] in
                    self?.updateFloatingCopyVisibility()
                }
                addSubview(floatBtn)
                floatingSideCopyButton = floatBtn
                positionFloatingCopyButton()
            }
        case let .executionGroup(group):
            buildExecutionGroupView(group: group, width: contentWidth, coordinator: coordinator)
        case let .processGroup(proc):
            buildProcessGroupView(proc: proc, width: contentWidth, coordinator: coordinator)
        case let .claudeProcessGroup(proc, _):
            buildClaudeProcessGroupView(proc: proc, width: contentWidth, coordinator: coordinator)
        case let .claudeMetadataGroup(meta, _):
            buildClaudeMetadataGroupView(meta: meta, width: contentWidth, coordinator: coordinator)
        case let .thinking(title, text, running):
            buildThinkingView(
                id: item.id,
                aliases: item.timelineAliases,
                title: title,
                text: text,
                running: running,
                width: contentWidth,
                coordinator: coordinator
            )
        case let .plan(markdown, statusText, fallbackTitle):
            buildPlanView(id: item.id, markdown: markdown, statusText: statusText, fallbackTitle: fallbackTitle, width: contentWidth, coordinator: coordinator)
        case let .question(header, questions):
            buildQuestionView(header: header, questions: questions, answers: nil, width: contentWidth, coordinator: coordinator)
        case let .questionResult(statusText, questions, answers):
            buildQuestionView(header: statusText, questions: questions, answers: answers, width: contentWidth, coordinator: coordinator)
        case let .subagent(sub, supported):
            buildSubagentRow(sub: sub, supported: supported, width: contentWidth, coordinator: coordinator)
        case let .historyCollapse(eyebrow, label, expanded):
            buildHistoryCollapseRow(eyebrow: eyebrow, label: label, expanded: expanded, width: contentWidth, coordinator: coordinator)
        case let .systemNotice(label, detail):
            buildSystemNoticeRow(label: label, detail: detail, width: contentWidth, palette: palette)
        }
    }

    private func buildUserView(
        item: ConversationRowItem,
        msg: UserMessageContent?,
        rawText: String,
        namesOnly: Bool,
        width: CGFloat,
        coordinator: ConversationTableCoordinator
    ) {
        let palette = coordinator.palette
        let body = msg?.text ?? rawText
        let proj = UserContextEngine.projectUserMessage(body, contextBlocks: msg?.contextBlocks)
        let segs = UserContextEngine.userMessageSegments(proj.text, contextBlocks: proj.contextBlocks)
        let identity = msg?.id ?? item.timelineId

        let rightAlignRow = NSStackView()
        rightAlignRow.orientation = .vertical
        rightAlignRow.alignment = .trailing
        rightAlignRow.spacing = 10
        rightAlignRow.translatesAutoresizingMaskIntoConstraints = false
        rightAlignRow.widthAnchor.constraint(equalToConstant: width).isActive = true

        if let attachments = msg?.attachments, !attachments.isEmpty {
            let attRow = NSStackView()
            attRow.orientation = .horizontal
            attRow.spacing = 8
            for att in attachments {
                if !namesOnly && att.kind == "image" {
                    let key = "\(coordinator.sessionScopeKey):\(msg?.id ?? item.id):\(att.id)"
                    let thumbBtn = ActionButton(frame: NSRect(x: 0, y: 0, width: 96, height: 96))
                    thumbBtn.wantsLayer = true
                    thumbBtn.layer?.cornerRadius = 12
                    thumbBtn.layer?.masksToBounds = true
                    thumbBtn.layer?.backgroundColor = palette.surface.nsColor.cgColor
                    thumbBtn.layer?.borderWidth = 1
                    thumbBtn.layer?.borderColor = palette.borderStrong.nsColor.cgColor
                    thumbBtn.translatesAutoresizingMaskIntoConstraints = false
                    thumbBtn.widthAnchor.constraint(equalToConstant: 96).isActive = true
                    thumbBtn.heightAnchor.constraint(equalToConstant: 96).isActive = true
                    if att.reason == nil, let img = coordinator.attachmentViewport.image(for: key) {
                        thumbBtn.image = img
                        thumbBtn.imageScaling = .scaleProportionallyUpOrDown
                        thumbBtn.title = ""
                        thumbBtn.toolTip = att.name
                        thumbBtn.onTap = { [weak coordinator] in
                            coordinator?.onPreviewImage?(att.name, img)
                        }
                    } else {
                        thumbBtn.image = LucideVectorCatalog.nsImage(
                            for: .imageOff,
                            size: 22,
                            color: palette.textMuted.nsColor
                        )
                        thumbBtn.imagePosition = .imageAbove
                        thumbBtn.font = DesignFonts.nsBodyFont(size: 11)
                        thumbBtn.contentTintColor = palette.textMuted.nsColor
                        thumbBtn.title = (att.reason != nil) ? "图片不可用" : "图片"
                        thumbBtn.toolTip = att.reason ?? att.name
                        thumbBtn.isEnabled = false
                    }
                    attRow.addArrangedSubview(thumbBtn)
                } else {
                    attRow.addArrangedSubview(makeFileAttachmentCard(att: att, namesOnly: namesOnly, palette: palette, coordinator: coordinator))
                }
            }
            rightAlignRow.addArrangedSubview(attRow)
        }

        let maxBubbleW = max(140, min(width * 0.85, 640))
        if !proj.contextBlocks.isEmpty {
            let ctxKey = "\(identity):context"
            let ctxAliases = ["\(item.timelineId):context"]
            let open = coordinator.disclosureStore.isOpen(
                scope: coordinator.sessionScopeKey,
                stateKey: ctxKey,
                aliases: ctxAliases,
                initiallyOpen: false
            )
            let ctxText = proj.contextBlocks.map { UserContextEngine.visibleContextText($0.text) }.joined(separator: "\n\n")
            rightAlignRow.addArrangedSubview(
                makeUserInstructionsCard(
                    stateKey: ctxKey,
                    aliases: ctxAliases,
                    open: open,
                    text: ctxText,
                    maxCardWidth: maxBubbleW,
                    palette: palette,
                    coordinator: coordinator
                )
            )
        }

        let para = NSMutableParagraphStyle()
        para.minimumLineHeight = 24.75 // 15pt * 1.65
        para.maximumLineHeight = 24.75
        let attrs: [NSAttributedString.Key: Any] = [
            .font: DesignFonts.nsBodyFont(size: 15),
            .foregroundColor: palette.textPrimary.nsColor,
            .paragraphStyle: para,
        ]
        let hPad: CGFloat = (width <= 500) ? 16 : 18
        let vPad: CGFloat = 12

        for seg in segs {
            switch seg {
            case let .text(_, t):
                let bubble = makeUserBubbleContainer(palette: palette)
                let attr = NSAttributedString(string: t, attributes: attrs)
                let textW = min(maxBubbleW - hPad * 2, max(36, ceil(attr.size().width) + 4))
                let textH = max(24, ReaderSelectableTextView.measureHeight(attributed: attr, width: textW))
                let tv = ReaderSelectableTextView(frame: NSRect(x: 0, y: 0, width: textW, height: textH))
                tv.applyAttributedText(attr, width: textW)
                tv.translatesAutoresizingMaskIntoConstraints = false
                bubble.addSubview(tv)
                NSLayoutConstraint.activate([
                    tv.leadingAnchor.constraint(equalTo: bubble.leadingAnchor, constant: hPad),
                    tv.trailingAnchor.constraint(equalTo: bubble.trailingAnchor, constant: -hPad),
                    tv.topAnchor.constraint(equalTo: bubble.topAnchor, constant: vPad),
                    tv.bottomAnchor.constraint(equalTo: bubble.bottomAnchor, constant: -vPad),
                    tv.widthAnchor.constraint(equalToConstant: textW),
                    tv.heightAnchor.constraint(equalToConstant: textH),
                ])
                rightAlignRow.addArrangedSubview(bubble)

            case let .reply(segId, q, a):
                let replyRow = NSStackView()
                replyRow.orientation = .horizontal
                replyRow.alignment = .centerY
                replyRow.spacing = 8

                let copyAnsBtn = IconFeedbackActionButton(
                    dimension: 30,
                    iconSize: 17,
                    idleIcon: .copy,
                    palette: palette,
                    toolTipText: "复制回答"
                ) { [weak self, weak coordinator] in
                    NSPasteboard.general.clearContents()
                    NSPasteboard.general.setString(a, forType: .string)
                    coordinator?.onCopyFeedback?("已复制回答")
                    self?.updateFloatingCopyVisibility()
                }
                copyAnsBtn.onFeedbackStateChanged = { [weak self] in
                    self?.updateFloatingCopyVisibility()
                }
                questionReplyCopyButtons.append(copyAnsBtn)

                let bubble = makeUserBubbleContainer(palette: palette)
                let innerStack = NSStackView()
                innerStack.orientation = .vertical
                innerStack.alignment = .leading
                innerStack.spacing = 6
                innerStack.translatesAutoresizingMaskIntoConstraints = false
                bubble.addSubview(innerStack)

                let trimmedQ = q.trimmingCharacters(in: .whitespacesAndNewlines)
                let availBubbleTextW = max(80, maxBubbleW - hPad * 2 - 38)
                var computedAnsW: CGFloat = 64
                let ansAttr = NSAttributedString(string: a, attributes: attrs)
                computedAnsW = min(availBubbleTextW, max(64, ceil(ansAttr.size().width) + 4))

                if !trimmedQ.isEmpty {
                    let qKey = "\(identity):\(segId):question"
                    let qAliases = ["\(item.timelineId):\(segId):question"]
                    let qOpen = coordinator.disclosureStore.isOpen(
                        scope: coordinator.sessionScopeKey,
                        stateKey: qKey,
                        aliases: qAliases,
                        initiallyOpen: false
                    )
                    let qSingleWidth = ceil(NSAttributedString(
                        string: trimmedQ,
                        attributes: [.font: DesignFonts.nsBodyFont(size: 13)]
                    ).size().width) + 26
                    let targetInnerW = min(availBubbleTextW, max(computedAnsW, qSingleWidth))

                    let qHeaderBox = ActionRowContainerView()
                    qHeaderBox.translatesAutoresizingMaskIntoConstraints = false
                    qHeaderBox.onTap = { [weak coordinator] in
                        coordinator?.toggleDisclosurePreservingAnchor(stateKey: qKey, aliases: qAliases, currentlyOpen: qOpen)
                    }
                    let qHeaderStack = NSStackView()
                    qHeaderStack.orientation = .horizontal
                    qHeaderStack.alignment = .centerY
                    qHeaderStack.spacing = 6
                    qHeaderStack.translatesAutoresizingMaskIntoConstraints = false
                    qHeaderBox.addSubview(qHeaderStack)

                    let qSummaryLbl = NSTextField(labelWithString: trimmedQ)
                    qSummaryLbl.font = DesignFonts.nsBodyFont(size: 13)
                    qSummaryLbl.textColor = palette.textSecondary.nsColor.withAlphaComponent(0.84)
                    qSummaryLbl.lineBreakMode = .byTruncatingTail
                    qSummaryLbl.setContentCompressionResistancePriority(.defaultLow, for: .horizontal)

                    let qChev = NSImageView(
                        image: LucideVectorCatalog.nsImage(
                            for: qOpen ? .chevronDown : .chevronRight,
                            size: 15,
                            color: (qOpen ? palette.accent : palette.textMuted).nsColor
                        )
                    )
                    qChev.translatesAutoresizingMaskIntoConstraints = false
                    NSLayoutConstraint.activate([
                        qChev.widthAnchor.constraint(equalToConstant: 15),
                        qChev.heightAnchor.constraint(equalToConstant: 15),
                        qHeaderBox.widthAnchor.constraint(equalToConstant: targetInnerW),
                        qHeaderBox.heightAnchor.constraint(equalToConstant: 22),
                        qHeaderStack.leadingAnchor.constraint(equalTo: qHeaderBox.leadingAnchor),
                        qHeaderStack.trailingAnchor.constraint(equalTo: qHeaderBox.trailingAnchor),
                        qHeaderStack.centerYAnchor.constraint(equalTo: qHeaderBox.centerYAnchor),
                    ])
                    qHeaderStack.addArrangedSubview(qSummaryLbl)
                    qHeaderStack.addArrangedSubview(NSView())
                    qHeaderStack.addArrangedSubview(qChev)
                    innerStack.addArrangedSubview(qHeaderBox)

                    if qOpen {
                        let qAttr = NSAttributedString(
                            string: trimmedQ,
                            attributes: [
                                .font: DesignFonts.nsBodyFont(size: 13),
                                .foregroundColor: palette.textSecondary.nsColor,
                            ]
                        )
                        let qH = max(20, ReaderSelectableTextView.measureHeight(attributed: qAttr, width: targetInnerW))
                        let qFull = ReaderSelectableTextView(frame: NSRect(x: 0, y: 0, width: targetInnerW, height: qH))
                        qFull.applyAttributedText(qAttr, width: targetInnerW)
                        qFull.translatesAutoresizingMaskIntoConstraints = false
                        NSLayoutConstraint.activate([
                            qFull.widthAnchor.constraint(equalToConstant: targetInnerW),
                            qFull.heightAnchor.constraint(equalToConstant: qH),
                        ])
                        innerStack.addArrangedSubview(qFull)
                    }
                    computedAnsW = max(computedAnsW, targetInnerW)
                }

                let ansH = max(24, ReaderSelectableTextView.measureHeight(attributed: ansAttr, width: computedAnsW))
                let ansTv = ReaderSelectableTextView(frame: NSRect(x: 0, y: 0, width: computedAnsW, height: ansH))
                ansTv.applyAttributedText(ansAttr, width: computedAnsW)
                ansTv.translatesAutoresizingMaskIntoConstraints = false
                NSLayoutConstraint.activate([
                    ansTv.widthAnchor.constraint(equalToConstant: computedAnsW),
                    ansTv.heightAnchor.constraint(equalToConstant: ansH),
                ])
                innerStack.addArrangedSubview(ansTv)

                NSLayoutConstraint.activate([
                    innerStack.leadingAnchor.constraint(equalTo: bubble.leadingAnchor, constant: hPad),
                    innerStack.trailingAnchor.constraint(equalTo: bubble.trailingAnchor, constant: -hPad),
                    innerStack.topAnchor.constraint(equalTo: bubble.topAnchor, constant: vPad),
                    innerStack.bottomAnchor.constraint(equalTo: bubble.bottomAnchor, constant: -vPad),
                ])

                replyRow.addArrangedSubview(copyAnsBtn)
                replyRow.addArrangedSubview(bubble)
                rightAlignRow.addArrangedSubview(replyRow)
            }
        }
        containerStack.addArrangedSubview(rightAlignRow)
    }

    private func makeUserBubbleContainer(palette: DesignPalette) -> NSView {
        let bubble = NSView()
        bubble.translatesAutoresizingMaskIntoConstraints = false
        bubble.wantsLayer = true
        bubble.layer?.cornerRadius = 20
        bubble.layer?.backgroundColor = palette.surface.nsColor.cgColor
        bubble.layer?.borderWidth = 1
        bubble.layer?.borderColor = palette.borderSubtle.nsColor.cgColor
        return bubble
    }

    private func makeUserInstructionsCard(
        stateKey: String,
        aliases: [String],
        open: Bool,
        text: String,
        maxCardWidth: CGFloat,
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
            coordinator?.toggleDisclosurePreservingAnchor(stateKey: stateKey, aliases: aliases, currentlyOpen: open)
        }
        let hStack = NSStackView()
        hStack.orientation = .horizontal
        hStack.alignment = .centerY
        hStack.spacing = 10
        hStack.translatesAutoresizingMaskIntoConstraints = false
        header.addSubview(hStack)

        let titleLbl = NSTextField(labelWithString: "上下文")
        titleLbl.font = DesignFonts.nsBodyFont(size: 14)
        titleLbl.textColor = palette.textPrimary.nsColor

        let chev = NSImageView(
            image: LucideVectorCatalog.nsImage(
                for: open ? .chevronDown : .chevronRight,
                size: 16,
                color: (open ? palette.accent : palette.textMuted).nsColor
            )
        )
        chev.translatesAutoresizingMaskIntoConstraints = false
        NSLayoutConstraint.activate([
            chev.widthAnchor.constraint(equalToConstant: 16),
            chev.heightAnchor.constraint(equalToConstant: 16),
            header.heightAnchor.constraint(equalToConstant: 30),
            hStack.leadingAnchor.constraint(equalTo: header.leadingAnchor),
            hStack.trailingAnchor.constraint(equalTo: header.trailingAnchor),
            hStack.centerYAnchor.constraint(equalTo: header.centerYAnchor),
        ])
        hStack.addArrangedSubview(titleLbl)
        if open {
            hStack.addArrangedSubview(NSView())
        }
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

        let innerW = max(120, maxCardWidth - 24)
        let para = NSMutableParagraphStyle()
        para.minimumLineHeight = 23.8 // 14pt * 1.7
        para.maximumLineHeight = 23.8
        let attr = NSAttributedString(
            string: text,
            attributes: [
                .font: DesignFonts.nsBodyFont(size: 14),
                .foregroundColor: palette.textPrimary.nsColor,
                .paragraphStyle: para,
            ]
        )
        let measuredH = max(24, ReaderSelectableTextView.measureHeight(attributed: attr, width: innerW))
        let bodyBoxH = min(300, max(32, measuredH + 20))

        let scroll = PassthroughInnerScrollView(allowsVerticalScroll: true)
        scroll.hasVerticalScroller = measuredH + 20 > bodyBoxH
        scroll.autohidesScrollers = true
        scroll.drawsBackground = false
        scroll.translatesAutoresizingMaskIntoConstraints = false

        let tv = ReaderSelectableTextView(frame: NSRect(x: 0, y: 0, width: innerW, height: max(bodyBoxH, measuredH + 16)))
        tv.textContainerInset = NSSize(width: 0, height: 8)
        tv.applyAttributedText(attr, width: innerW)
        scroll.documentView = tv
        card.addSubview(scroll)

        NSLayoutConstraint.activate([
            card.widthAnchor.constraint(equalToConstant: maxCardWidth),
            header.leadingAnchor.constraint(equalTo: card.leadingAnchor, constant: 12),
            header.trailingAnchor.constraint(equalTo: card.trailingAnchor, constant: -12),
            header.topAnchor.constraint(equalTo: card.topAnchor, constant: 4),
            scroll.leadingAnchor.constraint(equalTo: card.leadingAnchor, constant: 12),
            scroll.trailingAnchor.constraint(equalTo: card.trailingAnchor, constant: -12),
            scroll.topAnchor.constraint(equalTo: header.bottomAnchor, constant: 2),
            scroll.bottomAnchor.constraint(equalTo: card.bottomAnchor, constant: -2),
            scroll.heightAnchor.constraint(equalToConstant: bodyBoxH),
        ])
        return card
    }

    private func makeFileAttachmentCard(
        att: UserAttachment,
        namesOnly: Bool,
        palette: DesignPalette,
        coordinator: ConversationTableCoordinator
    ) -> NSView {
        let card = NSStackView()
        card.orientation = .horizontal
        card.alignment = .centerY
        card.spacing = 8
        card.wantsLayer = true
        card.layer?.cornerRadius = 12
        card.layer?.backgroundColor = palette.surface.nsColor.cgColor
        card.layer?.borderWidth = 1
        card.layer?.borderColor = palette.borderStrong.nsColor.cgColor
        card.edgeInsets = NSEdgeInsets(top: 8, left: 12, bottom: 8, right: 10)

        let iconView = NSImageView(
            image: LucideVectorCatalog.nsImage(for: .file, size: 18, color: palette.textSecondary.nsColor)
        )
        iconView.translatesAutoresizingMaskIntoConstraints = false
        NSLayoutConstraint.activate([
            iconView.widthAnchor.constraint(equalToConstant: 18),
            iconView.heightAnchor.constraint(equalToConstant: 18),
        ])
        card.addArrangedSubview(iconView)

        let textCol = NSStackView()
        textCol.orientation = .vertical
        textCol.alignment = .leading
        textCol.spacing = 2

        if !namesOnly, let rawPath = att.path, !rawPath.isEmpty {
            let linkBtn = ActionButton()
            let attrTitle = NSAttributedString(
                string: att.name,
                attributes: [
                    .font: DesignFonts.nsBodyFont(size: 14),
                    .foregroundColor: palette.accent.nsColor,
                    .underlineStyle: NSUnderlineStyle.single.rawValue,
                ]
            )
            linkBtn.attributedTitle = attrTitle
            linkBtn.toolTip = rawPath
            linkBtn.onTap = { [weak self, weak coordinator] in
                guard let self, let coordinator else { return }
                _ = self.handleActivatedHref(rawPath, coordinator: coordinator)
            }
            textCol.addArrangedSubview(linkBtn)
        } else {
            let nameLbl = NSTextField(labelWithString: att.name)
            nameLbl.font = DesignFonts.nsBodyFont(size: 14)
            nameLbl.textColor = palette.textPrimary.nsColor
            textCol.addArrangedSubview(nameLbl)
        }

        if let reason = att.reason, !reason.isEmpty {
            let reasonLbl = NSTextField(labelWithString: reason)
            reasonLbl.font = DesignFonts.nsBodyFont(size: 11)
            reasonLbl.textColor = palette.textMuted.nsColor
            textCol.addArrangedSubview(reasonLbl)
        }
        card.addArrangedSubview(textCol)

        if !namesOnly, let rawPath = att.path, !rawPath.isEmpty {
            let copyPathBtn = IconFeedbackActionButton(
                dimension: 28,
                iconSize: 14,
                idleIcon: .copy,
                palette: palette,
                toolTipText: "复制文件路径"
            ) { [weak coordinator] in
                NSPasteboard.general.clearContents()
                NSPasteboard.general.setString(rawPath, forType: .string)
                coordinator?.onCopyFeedback?("已复制路径")
            }
            card.addArrangedSubview(copyPathBtn)
        }

        return card
    }

    private func buildMarkdownViews(
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
            case let .prose(attr, isQuote):
                if isQuote {
                    let quoteWrap = NSView()
                    quoteWrap.translatesAutoresizingMaskIntoConstraints = false
                    quoteWrap.wantsLayer = true
                    quoteWrap.layer?.backgroundColor = palette.surface.nsColor.cgColor
                    quoteWrap.layer?.cornerRadius = 10
                    let bar = NSView()
                    bar.wantsLayer = true
                    bar.layer?.backgroundColor = palette.accent.nsColor.cgColor
                    bar.translatesAutoresizingMaskIntoConstraints = false

                    let innerW = max(100, width - 32)
                    let tv = makeProseTextView(attr: attr, width: innerW, coordinator: coordinator)
                    quoteWrap.addSubview(bar)
                    quoteWrap.addSubview(tv)
                    NSLayoutConstraint.activate([
                        quoteWrap.widthAnchor.constraint(equalToConstant: width),
                        bar.leadingAnchor.constraint(equalTo: quoteWrap.leadingAnchor),
                        bar.topAnchor.constraint(equalTo: quoteWrap.topAnchor),
                        bar.bottomAnchor.constraint(equalTo: quoteWrap.bottomAnchor),
                        bar.widthAnchor.constraint(equalToConstant: 3),
                        tv.leadingAnchor.constraint(equalTo: quoteWrap.leadingAnchor, constant: 14),
                        tv.trailingAnchor.constraint(equalTo: quoteWrap.trailingAnchor, constant: -14),
                        tv.topAnchor.constraint(equalTo: quoteWrap.topAnchor, constant: 8),
                        tv.bottomAnchor.constraint(equalTo: quoteWrap.bottomAnchor, constant: -8),
                    ])
                    destStack.addArrangedSubview(quoteWrap)
                } else {
                    let tv = makeProseTextView(attr: attr, width: width, coordinator: coordinator)
                    destStack.addArrangedSubview(tv)
                }

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

    private func handleActivatedHref(_ href: String, coordinator: ConversationTableCoordinator) -> Bool {
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

    private func makeProseTextView(
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

    private func makeCodeBlockView(
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

        let scroll = PassthroughInnerScrollView(allowsVerticalScroll: false)
        scroll.hasHorizontalScroller = true
        scroll.hasVerticalScroller = false
        scroll.autohidesScrollers = true
        scroll.drawsBackground = false
        scroll.translatesAutoresizingMaskIntoConstraints = false

        let codeHeight = ConversationTableCoordinator.measureCodeBlockHeight(code: normalizedCode)
        let docWidth = ConversationTableCoordinator.measureCodeDocumentWidth(code: normalizedCode, minWidth: width - 32)

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
            scroll.heightAnchor.constraint(equalToConstant: codeHeight),
        ])
        return box
    }

    private func makeTableView(
        headers: [NSAttributedString],
        rows: [[NSAttributedString]],
        width: CGFloat,
        palette: DesignPalette,
        coordinator: ConversationTableCoordinator
    ) -> NSView {
        let layout = ConversationTableCoordinator.measureTableLayout(headers: headers, rows: rows, maxWidth: width)
        let visibleW = min(width, layout.totalWidth)

        let scroll = PassthroughInnerScrollView(allowsVerticalScroll: false)
        scroll.hasHorizontalScroller = layout.totalWidth > visibleW
        scroll.hasVerticalScroller = false
        scroll.autohidesScrollers = true
        scroll.drawsBackground = false
        scroll.translatesAutoresizingMaskIntoConstraints = false

        let colCount = max(1, headers.count)
        let allRows = [headers] + rows
        let docView = NSView(frame: NSRect(x: 0, y: 0, width: layout.totalWidth, height: layout.totalHeight))
        var currentTopY = layout.totalHeight

        for (rIdx, cols) in allRows.enumerated() {
            let rowH = layout.rowHeights[rIdx]
            currentTopY -= rowH
            var currentX: CGFloat = 0
            for cIdx in 0 ..< colCount {
                let colW = layout.colWidths[cIdx]
                let cellRect = NSRect(x: currentX, y: currentTopY, width: colW, height: rowH)
                let cellBox = NSView(frame: cellRect)
                cellBox.wantsLayer = true
                cellBox.layer?.backgroundColor = (rIdx == 0 ? palette.surface : palette.background).nsColor.cgColor
                cellBox.layer?.borderWidth = 0.5
                cellBox.layer?.borderColor = palette.borderStrong.nsColor.cgColor

                let cellAttr = cIdx < cols.count ? cols[cIdx] : NSAttributedString(string: "")
                let innerW = max(36, colW - 24)
                let textH = max(18, ReaderSelectableTextView.measureHeight(attributed: cellAttr, width: innerW))
                let tv = ReaderSelectableTextView(
                    frame: NSRect(x: 12, y: max(4, (rowH - textH) * 0.5), width: innerW, height: textH)
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
        }
        docView.wantsLayer = true
        docView.layer?.borderWidth = 1
        docView.layer?.borderColor = palette.borderStrong.nsColor.cgColor
        scroll.documentView = docView
        NSLayoutConstraint.activate([
            scroll.widthAnchor.constraint(equalToConstant: visibleW),
            scroll.heightAnchor.constraint(equalToConstant: layout.totalHeight),
        ])
        return scroll
    }

    private func buildExecutionGroupView(
        group: ExecutionGroup,
        width: CGFloat,
        coordinator: ConversationTableCoordinator,
        into targetStack: NSStackView? = nil,
        nestedInRail: Bool = false
    ) {
        let destStack = targetStack ?? containerStack
        let palette = coordinator.palette
        let aliases = group.stateAliases
        let isOpen = coordinator.disclosureStore.isOpen(
            scope: coordinator.sessionScopeKey,
            stateKey: group.id,
            aliases: aliases,
            initiallyOpen: group.initiallyOpen
        )
        let header = group.summaryHeader
        let countMeta = "\(group.commands.count) \(group.kind == "tool" ? "项工具" : "条命令")"
            + (group.failedCount > 0 ? " · \(group.failedCount) 条失败" : "")

        let summaryRow = makeProcessSummaryRow(
            gutterIcon: header.icon,
            nestedInRail: nestedInRail,
            title: header.label,
            metaPill: countMeta,
            failureCount: 0,
            extraPills: [],
            running: group.running,
            open: isOpen,
            expandsChevronToTrailing: false,
            width: width,
            palette: palette
        ) { [weak coordinator] in
            coordinator?.toggleDisclosurePreservingAnchor(stateKey: group.id, aliases: aliases, currentlyOpen: isOpen)
        }
        destStack.addArrangedSubview(summaryRow)

        // Lazy child creation: do NOT build inner views when collapsed!
        guard isOpen else { return }
        let bodyWidth = max(120, width - 20)
        let cmdStack = makeExecutionCommandsList(
            commands: group.commands,
            width: bodyWidth,
            wrappedInSurfaceCard: false,
            palette: palette,
            coordinator: coordinator
        )
        destStack.addArrangedSubview(makeLeftRailContainer(content: cmdStack, width: width, palette: palette))
    }

    private func buildProcessGroupView(
        proc: UnifiedProcessGroup,
        width: CGFloat,
        coordinator: ConversationTableCoordinator
    ) {
        let palette = coordinator.palette
        let isOpen = coordinator.disclosureStore.isOpen(
            scope: coordinator.sessionScopeKey,
            stateKey: proc.stateKey,
            aliases: proc.stateAliases,
            initiallyOpen: proc.initiallyOpen
        )
        let summary = proc.summaryText
        let summaryRow = makeProcessSummaryRow(
            gutterIcon: proc.iconName,
            title: summary.primary,
            metaPill: summary.meta,
            failureCount: proc.failedCount,
            extraPills: [],
            running: proc.running,
            open: isOpen,
            expandsChevronToTrailing: false,
            width: width,
            palette: palette
        ) { [weak coordinator] in
            coordinator?.toggleDisclosurePreservingAnchor(stateKey: proc.stateKey, aliases: proc.stateAliases, currentlyOpen: isOpen)
        }
        containerStack.addArrangedSubview(summaryRow)

        guard isOpen else { return }
        let bodyWidth = max(120, width - 22)
        let wrapThinking = proc.toolCount > 0 || proc.thoughtCount > 1
        let stepsStack = NSStackView()
        stepsStack.orientation = .vertical
        stepsStack.alignment = .leading
        stepsStack.spacing = 8
        stepsStack.translatesAutoresizingMaskIntoConstraints = false

        for step in proc.steps {
            switch step {
            case let .thinking(id, sids, text, stepRunning, _):
                if wrapThinking {
                    stepsStack.addArrangedSubview(
                        makeInnerThinkingDisclosureView(
                            stateKey: "\(id):thinking",
                            aliases: sids.map { "\($0):thinking" },
                            markdown: text,
                            running: stepRunning,
                            width: bodyWidth,
                            palette: palette,
                            coordinator: coordinator
                        )
                    )
                } else {
                    stepsStack.addArrangedSubview(
                        makeThoughtProseView(markdown: text, width: bodyWidth, palette: palette, coordinator: coordinator)
                    )
                }
            case let .execution(group, _):
                stepsStack.addArrangedSubview(
                    makeExecutionCommandsList(
                        commands: group.commands,
                        width: bodyWidth,
                        wrappedInSurfaceCard: true,
                        palette: palette,
                        coordinator: coordinator
                    )
                )
            case let .detail(id, sids, title, detail, running, failed, _):
                let detailCmd = ExecutionCommand(
                    id: id,
                    sourceIds: sids,
                    title: title,
                    preview: nil,
                    status: failed ? .failed : (running ? .running : .completed),
                    instructionFile: false,
                    sections: [ExecutionSection(label: "", text: detail)]
                )
                stepsStack.addArrangedSubview(
                    makeExecutionCommandsList(
                        commands: [detailCmd],
                        width: bodyWidth,
                        wrappedInSurfaceCard: false,
                        palette: palette,
                        coordinator: coordinator
                    )
                )
            }
        }
        containerStack.addArrangedSubview(makeLeftRailContainer(content: stepsStack, width: width, palette: palette))
    }

    private func buildClaudeProcessGroupView(
        proc: ClaudeProcessGroup,
        width: CGFloat,
        coordinator: ConversationTableCoordinator
    ) {
        let palette = coordinator.palette
        let aliases = proc.sourceIds.map { "claude-process:\(proc.turnId):\($0)" }
        let isOpen = coordinator.disclosureStore.isOpen(
            scope: coordinator.sessionScopeKey,
            stateKey: proc.stateKey,
            aliases: aliases,
            initiallyOpen: false
        )
        var extra: [String] = []
        if proc.needsReply { extra.append("待回复") }
        if proc.hasPermission { extra.append(proc.needsPermission ? "待授权" : "授权记录") }
        if !proc.running && (proc.needsReply || proc.needsPermission) { extra.append("展开查看") }

        let summaryRow = makeProcessSummaryRow(
            gutterIcon: proc.iconName,
            title: proc.labelText,
            metaPill: proc.metaText,
            failureCount: proc.failedCount,
            extraPills: extra,
            running: proc.running,
            open: isOpen,
            expandsChevronToTrailing: false,
            width: width,
            palette: palette
        ) { [weak coordinator] in
            coordinator?.toggleDisclosurePreservingAnchor(stateKey: proc.stateKey, aliases: aliases, currentlyOpen: isOpen)
        }
        containerStack.addArrangedSubview(summaryRow)

        guard isOpen else { return }
        let bodyWidth = max(120, width - 22)
        let innerStack = buildClaudeEntriesStack(
            entries: proc.entries,
            width: bodyWidth,
            wrapThinkingDisclosures: proc.wrapsThinkingDisclosures,
            coordinator: coordinator
        )
        containerStack.addArrangedSubview(makeLeftRailContainer(content: innerStack, width: width, palette: palette))
    }

    private func buildClaudeMetadataGroupView(
        meta: ClaudeMetadataGroup,
        width: CGFloat,
        coordinator: ConversationTableCoordinator,
        into targetStack: NSStackView? = nil
    ) {
        let destStack = targetStack ?? containerStack
        let palette = coordinator.palette
        let aliases = meta.sourceIds.map { "claude-metadata:\($0)" }
        let isOpen = coordinator.disclosureStore.isOpen(
            scope: coordinator.sessionScopeKey,
            stateKey: meta.id,
            aliases: aliases,
            initiallyOpen: false
        )
        let summaryRow = makeProcessSummaryRow(
            gutterIcon: nil,
            title: meta.label,
            metaPill: nil,
            failureCount: 0,
            extraPills: [],
            running: false,
            open: isOpen,
            expandsChevronToTrailing: true,
            width: width,
            palette: palette
        ) { [weak coordinator] in
            coordinator?.toggleDisclosurePreservingAnchor(stateKey: meta.id, aliases: aliases, currentlyOpen: isOpen)
        }
        destStack.addArrangedSubview(summaryRow)

        guard isOpen else { return }
        let subItems = meta.entries.map { entry -> ClaudeEventRenderItem in
            switch entry {
            case let .item(it, key, sids): .item(item: it, key: key, sourceIds: sids)
            case let .group(g): .group(g)
            }
        }
        let bodyWidth = max(100, width - 22)
        let innerStack = buildClaudeEntriesStack(
            entries: subItems,
            width: bodyWidth,
            wrapThinkingDisclosures: false,
            coordinator: coordinator
        )
        destStack.addArrangedSubview(makeLeftRailContainer(content: innerStack, width: width, palette: palette))
    }

    private func buildClaudeEntriesStack(
        entries: [ClaudeEventRenderItem],
        width: CGFloat,
        wrapThinkingDisclosures: Bool,
        coordinator: ConversationTableCoordinator
    ) -> NSStackView {
        let palette = coordinator.palette
        let stack = NSStackView()
        stack.orientation = .vertical
        stack.alignment = .leading
        stack.spacing = 8
        stack.translatesAutoresizingMaskIntoConstraints = false

        for entry in entries {
            switch entry {
            case let .group(group):
                buildExecutionGroupView(group: group, width: width, coordinator: coordinator, into: stack, nestedInRail: true)
            case let .metadata(meta):
                buildClaudeMetadataGroupView(meta: meta, width: width, coordinator: coordinator, into: stack)
            case let .item(ev, key, sids):
                if ClaudeProcessGroupingEngine.isThoughtOrCommentary(ev) {
                    let markdown = ev.text ?? ""
                    guard !VisibleMarkdownEngine.visibleMarkdown(markdown).trimmingCharacters(in: .whitespacesAndNewlines).isEmpty else { continue }
                    if wrapThinkingDisclosures {
                        stack.addArrangedSubview(
                            makeInnerThinkingDisclosureView(
                                stateKey: "\(key):thinking",
                                aliases: sids.map { "\($0):thinking" },
                                markdown: markdown,
                                running: ev.kind == "thinking" && ev.status == "running",
                                width: width,
                                palette: palette,
                                coordinator: coordinator
                            )
                        )
                    } else {
                        stack.addArrangedSubview(makeThoughtProseView(markdown: markdown, width: width, palette: palette, coordinator: coordinator))
                    }
                } else if let detail = ev.detail, !detail.isEmpty {
                    let cmd = ExecutionCommand(
                        id: key,
                        sourceIds: sids,
                        title: ev.text ?? ev.role ?? "活动记录",
                        preview: nil,
                        status: ExecutionGroupingEngine.executionStatus(ev.status),
                        instructionFile: false,
                        sections: [ExecutionSection(label: "", text: detail)]
                    )
                    stack.addArrangedSubview(
                        makeExecutionCommandsList(
                            commands: [cmd],
                            width: width,
                            wrappedInSurfaceCard: false,
                            palette: palette,
                            coordinator: coordinator
                        )
                    )
                } else if let text = ev.text, !text.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty {
                    stack.addArrangedSubview(makeThoughtProseView(markdown: text, width: width, palette: palette, coordinator: coordinator))
                }
            }
        }
        return stack
    }

    private func buildThinkingView(
        id: String,
        aliases: [String],
        title: String,
        text: String,
        running: Bool,
        width: CGFloat,
        coordinator: ConversationTableCoordinator
    ) {
        let palette = coordinator.palette
        let open = coordinator.disclosureStore.isOpen(
            scope: coordinator.sessionScopeKey,
            stateKey: id,
            aliases: aliases,
            initiallyOpen: false
        )
        let summaryRow = makeProcessSummaryRow(
            gutterIcon: .brain,
            title: running ? "正在思考…" : title,
            metaPill: nil,
            failureCount: 0,
            extraPills: [],
            running: running,
            open: open,
            expandsChevronToTrailing: false,
            width: width,
            palette: palette
        ) { [weak coordinator] in
            coordinator?.toggleDisclosurePreservingAnchor(stateKey: id, aliases: aliases, currentlyOpen: open)
        }
        containerStack.addArrangedSubview(summaryRow)
        guard open else { return }
        let thoughtView = makeThoughtProseView(markdown: text, width: max(120, width - 22), palette: palette, coordinator: coordinator)
        containerStack.addArrangedSubview(makeLeftRailContainer(content: thoughtView, width: width, palette: palette))
    }

    public static func thoughtPreviewSnippet(_ markdown: String) -> String {
        let cleaned = VisibleMarkdownEngine.visibleMarkdown(markdown)
        for rawLine in cleaned.components(separatedBy: .newlines) {
            let trimmed = rawLine
                .replacingOccurrences(of: "^(#{1,6}\\s+|[-*+]\\s+|>\\s+|\\d+\\.\\s+)+", with: "", options: .regularExpression)
                .replacingOccurrences(of: "\\s+", with: " ", options: .regularExpression)
                .trimmingCharacters(in: .whitespacesAndNewlines)
            if !trimmed.isEmpty {
                let ns = trimmed as NSString
                return ns.length > 140 ? "\(ns.substring(to: 140))…" : trimmed
            }
        }
        return "思考内容"
    }

    private func makeInnerThinkingDisclosureView(
        stateKey: String,
        aliases: [String],
        markdown: String,
        running: Bool,
        width: CGFloat,
        palette: DesignPalette,
        coordinator: ConversationTableCoordinator
    ) -> NSView {
        let open = coordinator.disclosureStore.isOpen(
            scope: coordinator.sessionScopeKey,
            stateKey: stateKey,
            aliases: aliases,
            initiallyOpen: false
        )
        let itemCol = NSStackView()
        itemCol.orientation = .vertical
        itemCol.alignment = .leading
        itemCol.spacing = 6
        itemCol.translatesAutoresizingMaskIntoConstraints = false
        itemCol.widthAnchor.constraint(equalToConstant: width).isActive = true

        let header = HoverSummaryRowView(
            open: false,
            isEnabled: true,
            hoverFill: palette.hover.nsColor.withAlphaComponent(0.55),
            openFill: .clear
        )
        header.onTap = { [weak coordinator] in
            coordinator?.toggleDisclosurePreservingAnchor(stateKey: stateKey, aliases: aliases, currentlyOpen: open)
        }
        let hStack = NSStackView()
        hStack.orientation = .horizontal
        hStack.alignment = .centerY
        hStack.spacing = 8
        hStack.translatesAutoresizingMaskIntoConstraints = false
        header.addSubview(hStack)

        let brainIcon = NSImageView(
            image: LucideVectorCatalog.nsImage(for: .brain, size: 14, color: palette.accent.nsColor)
        )
        brainIcon.translatesAutoresizingMaskIntoConstraints = false
        NSLayoutConstraint.activate([
            brainIcon.widthAnchor.constraint(equalToConstant: 14),
            brainIcon.heightAnchor.constraint(equalToConstant: 14),
        ])
        hStack.addArrangedSubview(brainIcon)

        let titleAttr = NSMutableAttributedString(
            string: running ? "正在思考…" : "思考",
            attributes: [
                .font: DesignFonts.nsBodyFont(size: 12, weight: .medium),
                .foregroundColor: palette.textPrimary.nsColor,
            ]
        )
        let preview = Self.thoughtPreviewSnippet(markdown)
        if !preview.isEmpty {
            titleAttr.append(
                NSAttributedString(
                    string: " \(preview)",
                    attributes: [
                        .font: DesignFonts.nsBodyFont(size: 12),
                        .foregroundColor: palette.textMuted.nsColor,
                    ]
                )
            )
        }
        let titleLbl = NSTextField(labelWithAttributedString: titleAttr)
        titleLbl.lineBreakMode = .byTruncatingTail
        titleLbl.setContentCompressionResistancePriority(.defaultLow, for: .horizontal)
        hStack.addArrangedSubview(titleLbl)

        hStack.addArrangedSubview(NSView())
        if running {
            hStack.addArrangedSubview(
                AppKitRunningRingView(
                    size: 13,
                    trackColor: palette.borderStrong.nsColor,
                    accentColor: palette.accent.nsColor
                )
            )
        }

        let chev = NSImageView(
            image: LucideVectorCatalog.nsImage(
                for: open ? .chevronDown : .chevronRight,
                size: 16,
                color: (open ? palette.accent : palette.textMuted).nsColor
            )
        )
        chev.translatesAutoresizingMaskIntoConstraints = false
        NSLayoutConstraint.activate([
            chev.widthAnchor.constraint(equalToConstant: 16),
            chev.heightAnchor.constraint(equalToConstant: 16),
            header.widthAnchor.constraint(equalToConstant: width),
            header.heightAnchor.constraint(equalToConstant: 32),
            hStack.leadingAnchor.constraint(equalTo: header.leadingAnchor, constant: 4),
            hStack.trailingAnchor.constraint(equalTo: header.trailingAnchor, constant: -4),
            hStack.centerYAnchor.constraint(equalTo: header.centerYAnchor),
        ])
        hStack.addArrangedSubview(chev)
        itemCol.addArrangedSubview(header)

        if open {
            itemCol.addArrangedSubview(
                makeThoughtProseView(markdown: markdown, width: width, palette: palette, coordinator: coordinator)
            )
        }
        return itemCol
    }

    private func makeThoughtProseView(
        markdown: String,
        width: CGFloat,
        palette: DesignPalette,
        coordinator: ConversationTableCoordinator
    ) -> NSView {
        let attr = ConversationTableCoordinator.renderThoughtAttributedString(markdown: markdown, palette: palette)
        let h = max(22, ReaderSelectableTextView.measureHeight(attributed: attr, width: width))
        let tv = ReaderSelectableTextView(frame: NSRect(x: 0, y: 0, width: width, height: h))
        tv.applyAttributedText(attr, width: width)
        tv.translatesAutoresizingMaskIntoConstraints = false
        NSLayoutConstraint.activate([
            tv.widthAnchor.constraint(equalToConstant: width),
            tv.heightAnchor.constraint(equalToConstant: h),
        ])
        tv.onLinkActivated = { [weak self, weak coordinator] href in
            guard let self, let coordinator else { return false }
            return self.handleActivatedHref(href, coordinator: coordinator)
        }
        return tv
    }

    private func buildPlanView(
        id: String,
        markdown: String,
        statusText: String,
        fallbackTitle: String,
        width: CGFloat,
        coordinator: ConversationTableCoordinator
    ) {
        let palette = coordinator.palette
        let cleanedPlan = VisibleMarkdownEngine.visibleMarkdown(markdown)

        let planCard = NSView()
        planCard.wantsLayer = true
        planCard.layer?.cornerRadius = 12
        planCard.layer?.masksToBounds = true
        planCard.layer?.backgroundColor = palette.surface.nsColor.cgColor
        planCard.layer?.borderWidth = 1
        planCard.layer?.borderColor = palette.borderSubtle.nsColor.cgColor

        let leftBar = NSView()
        leftBar.wantsLayer = true
        leftBar.layer?.backgroundColor = palette.accent.nsColor.cgColor
        leftBar.translatesAutoresizingMaskIntoConstraints = false
        planCard.addSubview(leftBar)

        let innerStack = NSStackView()
        innerStack.orientation = .vertical
        innerStack.alignment = .leading
        innerStack.spacing = 10
        innerStack.translatesAutoresizingMaskIntoConstraints = false
        planCard.addSubview(innerStack)

        let innerWidth = max(120, width - 35)
        let headerRow = NSStackView()
        headerRow.orientation = .horizontal
        headerRow.alignment = .centerY
        headerRow.spacing = 6
        headerRow.widthAnchor.constraint(equalToConstant: innerWidth).isActive = true

        let badgeTitle = NSTextField(labelWithString: "Proposed Plan")
        badgeTitle.font = DesignFonts.nsBodyFont(size: 12, weight: .medium)
        badgeTitle.textColor = palette.textMuted.nsColor

        let badgeStatus = NSTextField(labelWithString: statusText)
        badgeStatus.font = DesignFonts.nsBodyFont(size: 12)
        badgeStatus.textColor = palette.textMuted.nsColor

        let copyBtn = IconFeedbackActionButton(
            dimension: 30,
            iconSize: 17,
            idleIcon: .copy,
            palette: palette,
            toolTipText: "复制计划"
        ) { [weak coordinator] in
            NSPasteboard.general.clearContents()
            NSPasteboard.general.setString(cleanedPlan, forType: .string)
            coordinator?.onCopyFeedback?("已复制")
        }

        let saveBtn = IconFeedbackActionButton(
            dimension: 30,
            iconSize: 17,
            idleIcon: .download,
            palette: palette,
            toolTipText: "下载计划 Markdown"
        ) { [weak coordinator] in
            guard let coordinator else { return }
            let filename = FileLinksAndPlanExport.planFilename(markdown: cleanedPlan, fallbackTitle: fallbackTitle)
            Task { @MainActor in
                struct Wrap: Encodable { let request: PlanSaveRequest }
                do {
                    let _: PlanSaveResult = try await coordinator.store.rpc(
                        "plans.save",
                        args: Wrap(request: PlanSaveRequest(filename: filename, markdown: cleanedPlan)),
                        localOnly: true
                    )
                    coordinator.onCopyFeedback?("已保存到下载文件夹")
                } catch {
                    coordinator.onCopyFeedback?("保存失败，请重试。")
                }
            }
        }

        headerRow.addArrangedSubview(badgeTitle)
        headerRow.addArrangedSubview(badgeStatus)
        headerRow.addArrangedSubview(NSView())
        headerRow.addArrangedSubview(copyBtn)
        headerRow.addArrangedSubview(saveBtn)
        innerStack.addArrangedSubview(headerRow)

        let segs = GfmLayoutCache.shared.getOrRender(
            markdown: cleanedPlan,
            foldInstructions: false,
            scheme: coordinator.scheme,
            workspace: coordinator.workspace,
            transport: coordinator.store.transport
        )
        for seg in segs {
            switch seg {
            case let .prose(attr, _):
                innerStack.addArrangedSubview(makeProseTextView(attr: attr, width: innerWidth, coordinator: coordinator))
            case let .codeBlock(lang, code):
                innerStack.addArrangedSubview(makeCodeBlockView(lang: lang, code: code, width: innerWidth, coordinator: coordinator))
            case let .table(_, headers, rows):
                innerStack.addArrangedSubview(makeTableView(headers: headers, rows: rows, width: innerWidth, palette: palette, coordinator: coordinator))
            case .thematicBreak:
                let rule = NSView()
                rule.wantsLayer = true
                rule.layer?.backgroundColor = palette.borderSubtle.nsColor.cgColor
                rule.translatesAutoresizingMaskIntoConstraints = false
                NSLayoutConstraint.activate([
                    rule.widthAnchor.constraint(equalToConstant: innerWidth),
                    rule.heightAnchor.constraint(equalToConstant: 1),
                ])
                innerStack.addArrangedSubview(rule)
            case .instructionsFold:
                break
            }
        }

        planCard.translatesAutoresizingMaskIntoConstraints = false
        NSLayoutConstraint.activate([
            planCard.widthAnchor.constraint(equalToConstant: width),
            leftBar.leadingAnchor.constraint(equalTo: planCard.leadingAnchor),
            leftBar.topAnchor.constraint(equalTo: planCard.topAnchor),
            leftBar.bottomAnchor.constraint(equalTo: planCard.bottomAnchor),
            leftBar.widthAnchor.constraint(equalToConstant: 3),
            innerStack.leadingAnchor.constraint(equalTo: planCard.leadingAnchor, constant: 19),
            innerStack.trailingAnchor.constraint(equalTo: planCard.trailingAnchor, constant: -16),
            innerStack.topAnchor.constraint(equalTo: planCard.topAnchor, constant: 14),
            innerStack.bottomAnchor.constraint(equalTo: planCard.bottomAnchor, constant: -14),
        ])
        containerStack.addArrangedSubview(planCard)
    }

    private func buildQuestionView(
        header: String,
        questions: [UserInputQuestion],
        answers: [UserInputAnswer]?,
        width: CGFloat,
        coordinator: ConversationTableCoordinator
    ) {
        let palette = coordinator.palette
        let card = NSView()
        card.translatesAutoresizingMaskIntoConstraints = false
        card.wantsLayer = true
        card.layer?.cornerRadius = 12
        card.layer?.backgroundColor = palette.surface.nsColor.cgColor
        card.layer?.borderWidth = 1
        card.layer?.borderColor = palette.borderSubtle.nsColor.cgColor

        let innerW = max(120, width - 32)
        let innerStack = NSStackView()
        innerStack.orientation = .vertical
        innerStack.alignment = .leading
        innerStack.spacing = 10
        innerStack.translatesAutoresizingMaskIntoConstraints = false
        card.addSubview(innerStack)

        let metaRow = NSStackView()
        metaRow.orientation = .horizontal
        metaRow.alignment = .centerY
        metaRow.spacing = 8
        let metaPrimary = NSTextField(labelWithString: "Questions")
        metaPrimary.font = DesignFonts.nsBodyFont(size: 12, weight: .medium)
        metaPrimary.textColor = palette.textMuted.nsColor
        let metaSub = NSTextField(labelWithString: header)
        metaSub.font = DesignFonts.nsBodyFont(size: 12)
        metaSub.textColor = palette.textMuted.nsColor
        metaRow.addArrangedSubview(metaPrimary)
        metaRow.addArrangedSubview(metaSub)
        innerStack.addArrangedSubview(metaRow)

        for q in questions {
            let qLbl = NSTextField(wrappingLabelWithString: q.question)
            qLbl.font = DesignFonts.nsBodyFont(size: 14, weight: .semibold)
            qLbl.textColor = palette.textPrimary.nsColor
            qLbl.preferredMaxLayoutWidth = innerW
            innerStack.addArrangedSubview(qLbl)

            let optsStack = NSStackView()
            optsStack.orientation = .vertical
            optsStack.alignment = .leading
            optsStack.spacing = 0
            optsStack.translatesAutoresizingMaskIntoConstraints = false
            optsStack.widthAnchor.constraint(equalToConstant: innerW).isActive = true

            for (idx, opt) in q.options.enumerated() {
                let optBox = NSView()
                optBox.translatesAutoresizingMaskIntoConstraints = false
                let numLbl = NSTextField(labelWithString: "\(idx + 1)")
                numLbl.font = DesignFonts.nsBodyFont(size: 13)
                numLbl.textColor = palette.textMuted.nsColor
                numLbl.translatesAutoresizingMaskIntoConstraints = false

                let textCol = NSStackView()
                textCol.orientation = .vertical
                textCol.alignment = .leading
                textCol.spacing = 4
                textCol.translatesAutoresizingMaskIntoConstraints = false

                let optTitle = NSTextField(wrappingLabelWithString: opt.label)
                optTitle.font = DesignFonts.nsBodyFont(size: 14, weight: .semibold)
                optTitle.textColor = palette.textPrimary.nsColor
                optTitle.preferredMaxLayoutWidth = max(80, innerW - 32)
                textCol.addArrangedSubview(optTitle)

                if let desc = opt.description, !desc.isEmpty {
                    let descLbl = NSTextField(wrappingLabelWithString: desc)
                    descLbl.font = DesignFonts.nsBodyFont(size: 12)
                    descLbl.textColor = palette.textMuted.nsColor
                    descLbl.preferredMaxLayoutWidth = max(80, innerW - 32)
                    textCol.addArrangedSubview(descLbl)
                }

                optBox.addSubview(numLbl)
                optBox.addSubview(textCol)
                NSLayoutConstraint.activate([
                    optBox.widthAnchor.constraint(equalToConstant: innerW),
                    numLbl.leadingAnchor.constraint(equalTo: optBox.leadingAnchor),
                    numLbl.topAnchor.constraint(equalTo: optBox.topAnchor, constant: 10),
                    numLbl.widthAnchor.constraint(equalToConstant: 24),
                    textCol.leadingAnchor.constraint(equalTo: optBox.leadingAnchor, constant: 32),
                    textCol.trailingAnchor.constraint(equalTo: optBox.trailingAnchor),
                    textCol.topAnchor.constraint(equalTo: optBox.topAnchor, constant: 10),
                    textCol.bottomAnchor.constraint(equalTo: optBox.bottomAnchor, constant: -10),
                ])

                if idx < q.options.count - 1 {
                    let sep = NSView()
                    sep.wantsLayer = true
                    sep.layer?.backgroundColor = palette.borderSubtle.nsColor.cgColor
                    sep.translatesAutoresizingMaskIntoConstraints = false
                    optBox.addSubview(sep)
                    NSLayoutConstraint.activate([
                        sep.leadingAnchor.constraint(equalTo: optBox.leadingAnchor),
                        sep.trailingAnchor.constraint(equalTo: optBox.trailingAnchor),
                        sep.bottomAnchor.constraint(equalTo: optBox.bottomAnchor),
                        sep.heightAnchor.constraint(equalToConstant: 1),
                    ])
                }
                optsStack.addArrangedSubview(optBox)
            }
            innerStack.addArrangedSubview(optsStack)
        }

        if let answers {
            if !answers.isEmpty {
                let ansList = NSStackView()
                ansList.orientation = .vertical
                ansList.alignment = .leading
                ansList.spacing = 8
                ansList.translatesAutoresizingMaskIntoConstraints = false
                for ans in answers {
                    let ansBox = NSView()
                    ansBox.translatesAutoresizingMaskIntoConstraints = false
                    ansBox.wantsLayer = true
                    ansBox.layer?.cornerRadius = 10
                    ansBox.layer?.backgroundColor = palette.elevated.nsColor.cgColor
                    ansBox.layer?.borderWidth = 1
                    ansBox.layer?.borderColor = palette.borderSubtle.nsColor.cgColor

                    let qIdLbl = NSTextField(labelWithString: ans.questionId)
                    qIdLbl.font = DesignFonts.nsBodyFont(size: 12)
                    qIdLbl.textColor = palette.textMuted.nsColor
                    qIdLbl.lineBreakMode = .byTruncatingTail
                    qIdLbl.translatesAutoresizingMaskIntoConstraints = false

                    let ansValText = ans.answers.isEmpty ? "未回答" : ans.answers.joined(separator: ", ")
                    let ansValLbl = NSTextField(wrappingLabelWithString: ansValText)
                    ansValLbl.font = DesignFonts.nsBodyFont(size: 13, weight: .semibold)
                    ansValLbl.textColor = palette.textPrimary.nsColor
                    let rightColW = max(80, innerW - 150)
                    ansValLbl.preferredMaxLayoutWidth = rightColW
                    ansValLbl.translatesAutoresizingMaskIntoConstraints = false

                    ansBox.addSubview(qIdLbl)
                    ansBox.addSubview(ansValLbl)
                    NSLayoutConstraint.activate([
                        ansBox.widthAnchor.constraint(equalToConstant: innerW),
                        qIdLbl.leadingAnchor.constraint(equalTo: ansBox.leadingAnchor, constant: 12),
                        qIdLbl.widthAnchor.constraint(equalToConstant: min(124, innerW * 0.34)),
                        qIdLbl.topAnchor.constraint(equalTo: ansBox.topAnchor, constant: 10),
                        ansValLbl.leadingAnchor.constraint(equalTo: qIdLbl.trailingAnchor, constant: 10),
                        ansValLbl.trailingAnchor.constraint(equalTo: ansBox.trailingAnchor, constant: -12),
                        ansValLbl.topAnchor.constraint(equalTo: ansBox.topAnchor, constant: 10),
                        ansValLbl.bottomAnchor.constraint(equalTo: ansBox.bottomAnchor, constant: -10),
                    ])
                    ansList.addArrangedSubview(ansBox)
                }
                innerStack.addArrangedSubview(ansList)
            } else {
                let pLbl = NSTextField(labelWithString: "Questions answered")
                pLbl.font = DesignFonts.nsBodyFont(size: 13)
                pLbl.textColor = palette.textMuted.nsColor
                innerStack.addArrangedSubview(pLbl)
            }
        }

        NSLayoutConstraint.activate([
            card.widthAnchor.constraint(equalToConstant: width),
            innerStack.leadingAnchor.constraint(equalTo: card.leadingAnchor, constant: 16),
            innerStack.trailingAnchor.constraint(equalTo: card.trailingAnchor, constant: -16),
            innerStack.topAnchor.constraint(equalTo: card.topAnchor, constant: 14),
            innerStack.bottomAnchor.constraint(equalTo: card.bottomAnchor, constant: -14),
        ])
        containerStack.addArrangedSubview(card)
    }

    public static func subagentActivityVerb(_ sub: SubagentActivity) -> String {
        if let ek = sub.eventKind {
            switch ek {
            case "started": return "开始工作"
            case "completed": return "已完成"
            case "interrupted": return "已中断"
            case "interacted": return "已交互"
            default: break
            }
        }
        switch sub.status {
        case "creating": return "正在创建"
        case "running": return "运行中"
        case "completed": return "已完成"
        case "failed": return "失败"
        case "interrupted": return "已中断"
        default: return "状态未知"
        }
    }

    private func buildSubagentRow(
        sub: SubagentActivity,
        supported: Bool,
        width: CGFloat,
        coordinator: ConversationTableCoordinator
    ) {
        let palette = coordinator.palette
        let available = supported && sub.available && (sub.agentId?.isEmpty == false)
        let reason = supported ? sub.unavailableReason : "请更新当前机器服务以查看子智能体"

        let wrapper = NSStackView()
        wrapper.orientation = .vertical
        wrapper.alignment = .leading
        wrapper.spacing = 4
        wrapper.clipsToBounds = false
        wrapper.translatesAutoresizingMaskIntoConstraints = false
        wrapper.widthAnchor.constraint(equalToConstant: width).isActive = true

        let rowBox = HoverSummaryRowView(
            open: false,
            isEnabled: available,
            hoverFill: palette.hover.nsColor.withAlphaComponent(0.5),
            openFill: .clear
        )
        rowBox.clipsToBounds = false
        rowBox.toolTip = reason
        rowBox.onTap = { [weak coordinator] in
            if available {
                coordinator?.onOpenSubagent?(sub)
            }
        }

        let botIcon = NSImageView(
            image: LucideVectorCatalog.nsImage(for: .bot, size: 16, color: palette.textSecondary.nsColor)
        )
        botIcon.translatesAutoresizingMaskIntoConstraints = false
        rowBox.addSubview(botIcon)

        let rowStack = NSStackView()
        rowStack.orientation = .horizontal
        rowStack.alignment = .centerY
        rowStack.spacing = 8
        rowStack.translatesAutoresizingMaskIntoConstraints = false
        rowBox.addSubview(rowStack)

        let nameLbl = NSTextField(labelWithString: VisibleMarkdownEngine.visibleMarkdown(sub.name))
        nameLbl.font = DesignFonts.nsBodyFont(size: 13, weight: .medium)
        nameLbl.textColor = (available ? palette.textMuted : palette.textMuted).nsColor
        nameLbl.lineBreakMode = .byTruncatingTail
        nameLbl.setContentCompressionResistancePriority(.defaultLow, for: .horizontal)
        rowStack.addArrangedSubview(nameLbl)

        if sub.status == "running" && (sub.eventKind == nil || sub.eventKind == "started") {
            let ring = AppKitRunningRingView(
                size: 13,
                trackColor: palette.borderStrong.nsColor,
                accentColor: palette.accent.nsColor
            )
            rowStack.addArrangedSubview(ring)
        }

        let verbLbl = NSTextField(labelWithString: Self.subagentActivityVerb(sub))
        verbLbl.font = DesignFonts.nsBodyFont(size: 12)
        verbLbl.textColor = (sub.status == "failed" ? palette.errFg : palette.textMuted).nsColor
        rowStack.addArrangedSubview(verbLbl)

        if available {
            let chev = NSImageView(
                image: LucideVectorCatalog.nsImage(for: .chevronRight, size: 15, color: palette.textMuted.nsColor)
            )
            chev.translatesAutoresizingMaskIntoConstraints = false
            NSLayoutConstraint.activate([
                chev.widthAnchor.constraint(equalToConstant: 15),
                chev.heightAnchor.constraint(equalToConstant: 15),
            ])
            rowStack.addArrangedSubview(chev)
        }
        rowStack.addArrangedSubview(NSView())

        NSLayoutConstraint.activate([
            rowBox.widthAnchor.constraint(equalToConstant: width),
            rowBox.heightAnchor.constraint(equalToConstant: 30),
            botIcon.leadingAnchor.constraint(equalTo: rowBox.leadingAnchor, constant: -24),
            botIcon.centerYAnchor.constraint(equalTo: rowBox.centerYAnchor),
            botIcon.widthAnchor.constraint(equalToConstant: 16),
            botIcon.heightAnchor.constraint(equalToConstant: 16),
            rowStack.leadingAnchor.constraint(equalTo: rowBox.leadingAnchor),
            rowStack.trailingAnchor.constraint(equalTo: rowBox.trailingAnchor),
            rowStack.centerYAnchor.constraint(equalTo: rowBox.centerYAnchor),
        ])
        wrapper.addArrangedSubview(rowBox)

        if !available, let reason, !reason.isEmpty {
            let reasonLbl = NSTextField(labelWithString: reason)
            reasonLbl.font = DesignFonts.nsBodyFont(size: 11)
            reasonLbl.textColor = palette.textMuted.nsColor
            reasonLbl.lineBreakMode = .byTruncatingTail
            reasonLbl.widthAnchor.constraint(equalToConstant: width).isActive = true
            wrapper.addArrangedSubview(reasonLbl)
        }

        containerStack.addArrangedSubview(wrapper)
    }

    private func buildHistoryCollapseRow(
        eyebrow: String,
        label: String,
        expanded: Bool,
        width: CGFloat,
        coordinator: ConversationTableCoordinator
    ) {
        let palette = coordinator.palette
        let card = NSView()
        card.translatesAutoresizingMaskIntoConstraints = false
        card.wantsLayer = true
        card.layer?.cornerRadius = 12
        card.layer?.backgroundColor = palette.surface.nsColor.cgColor
        card.layer?.borderWidth = 1
        card.layer?.borderColor = palette.borderSubtle.nsColor.cgColor

        let textCol = NSStackView()
        textCol.orientation = .vertical
        textCol.alignment = .leading
        textCol.spacing = 4
        textCol.translatesAutoresizingMaskIntoConstraints = false

        let eyeLbl = NSTextField(labelWithString: eyebrow)
        eyeLbl.font = DesignFonts.nsBodyFont(size: 12)
        eyeLbl.textColor = palette.textMuted.nsColor

        let mainLbl = NSTextField(labelWithString: label)
        mainLbl.font = DesignFonts.nsBodyFont(size: 14, weight: .semibold)
        mainLbl.textColor = palette.textPrimary.nsColor
        mainLbl.lineBreakMode = .byTruncatingTail
        textCol.addArrangedSubview(eyeLbl)
        textCol.addArrangedSubview(mainLbl)

        let btn = ActionButton()
        btn.title = expanded ? "已显示全部" : "显示全部历史"
        btn.isEnabled = !expanded
        btn.font = DesignFonts.nsBodyFont(size: 13, weight: .medium)
        btn.contentTintColor = palette.textPrimary.nsColor
        btn.wantsLayer = true
        btn.layer?.cornerRadius = 8
        btn.layer?.backgroundColor = palette.elevated.nsColor.cgColor
        btn.layer?.borderWidth = 1
        btn.layer?.borderColor = palette.borderStrong.nsColor.cgColor
        btn.translatesAutoresizingMaskIntoConstraints = false
        let btnW = ceil(NSAttributedString(string: btn.title, attributes: [.font: btn.font!]).size().width) + 28
        NSLayoutConstraint.activate([
            btn.widthAnchor.constraint(equalToConstant: max(108, btnW)),
            btn.heightAnchor.constraint(equalToConstant: 35),
        ])
        btn.onTap = { [weak coordinator] in
            coordinator?.onShowAllHistory?()
        }

        card.addSubview(textCol)
        card.addSubview(btn)
        NSLayoutConstraint.activate([
            card.widthAnchor.constraint(equalToConstant: width),
            card.heightAnchor.constraint(equalToConstant: 64),
            textCol.leadingAnchor.constraint(equalTo: card.leadingAnchor, constant: 16),
            textCol.centerYAnchor.constraint(equalTo: card.centerYAnchor),
            textCol.trailingAnchor.constraint(lessThanOrEqualTo: btn.leadingAnchor, constant: -12),
            btn.trailingAnchor.constraint(equalTo: card.trailingAnchor, constant: -16),
            btn.centerYAnchor.constraint(equalTo: card.centerYAnchor),
        ])
        containerStack.addArrangedSubview(card)
    }

    private func buildSystemNoticeRow(label: String, detail: String, width: CGFloat, palette: DesignPalette) {
        let lbl = NSTextField(labelWithString: "\(label): \(detail)")
        lbl.font = DesignFonts.nsBodyFont(size: 12)
        lbl.textColor = palette.textMuted.nsColor
        lbl.lineBreakMode = .byTruncatingTail
        lbl.widthAnchor.constraint(equalToConstant: width).isActive = true
        containerStack.addArrangedSubview(lbl)
    }

    private func makeProcessSummaryRow(
        gutterIcon: LucideIconName?,
        nestedInRail: Bool = false,
        title: String,
        metaPill: String?,
        failureCount: Int,
        extraPills: [String],
        running: Bool,
        open: Bool,
        expandsChevronToTrailing: Bool,
        width: CGFloat,
        palette: DesignPalette,
        action: @escaping () -> Void
    ) -> NSView {
        let row = HoverSummaryRowView(
            open: open,
            isEnabled: true,
            hoverFill: palette.surface.nsColor,
            openFill: palette.surface.nsColor
        )
        row.clipsToBounds = false
        row.onTap = action

        if let gutterIcon, !nestedInRail {
            let iconView = NSImageView(
                image: LucideVectorCatalog.nsImage(for: gutterIcon, size: 15, color: palette.accent.nsColor)
            )
            iconView.translatesAutoresizingMaskIntoConstraints = false
            row.addSubview(iconView)
            NSLayoutConstraint.activate([
                iconView.leadingAnchor.constraint(equalTo: row.leadingAnchor, constant: -20),
                iconView.centerYAnchor.constraint(equalTo: row.centerYAnchor),
                iconView.widthAnchor.constraint(equalToConstant: 15),
                iconView.heightAnchor.constraint(equalToConstant: 15),
            ])
        }

        let hStack = NSStackView()
        hStack.orientation = .horizontal
        hStack.alignment = .centerY
        hStack.spacing = 8
        hStack.translatesAutoresizingMaskIntoConstraints = false
        row.addSubview(hStack)

        if let gutterIcon, nestedInRail {
            let inlineIcon = NSImageView(
                image: LucideVectorCatalog.nsImage(for: gutterIcon, size: 14, color: palette.accent.nsColor)
            )
            inlineIcon.translatesAutoresizingMaskIntoConstraints = false
            NSLayoutConstraint.activate([
                inlineIcon.widthAnchor.constraint(equalToConstant: 14),
                inlineIcon.heightAnchor.constraint(equalToConstant: 14),
            ])
            hStack.addArrangedSubview(inlineIcon)
        }

        let titleLbl = NSTextField(labelWithString: title)
        titleLbl.font = DesignFonts.nsBodyFont(size: 13, weight: .medium)
        titleLbl.textColor = (open ? palette.textPrimary : palette.textSecondary).nsColor
        titleLbl.lineBreakMode = .byTruncatingTail
        titleLbl.setContentCompressionResistancePriority(.defaultLow, for: .horizontal)
        hStack.addArrangedSubview(titleLbl)

        if let metaPill, !metaPill.isEmpty {
            hStack.addArrangedSubview(
                makeCapsulePill(
                    text: metaPill,
                    textColor: palette.textMuted.nsColor,
                    fill: palette.elevated.nsColor,
                    border: palette.borderStrong.nsColor,
                    semibold: false
                )
            )
        }

        if failureCount > 0 {
            hStack.addArrangedSubview(
                makeCapsulePill(
                    text: "\(failureCount) 项失败",
                    textColor: palette.errFg.nsColor,
                    fill: palette.errBg.nsColor,
                    border: palette.errBorder.nsColor,
                    semibold: true
                )
            )
        }

        for extra in extraPills {
            hStack.addArrangedSubview(
                makeCapsulePill(
                    text: extra,
                    textColor: palette.textMuted.nsColor,
                    fill: palette.elevated.nsColor,
                    border: palette.borderStrong.nsColor,
                    semibold: false
                )
            )
        }

        if running || expandsChevronToTrailing {
            hStack.addArrangedSubview(NSView())
        }
        if running {
            hStack.addArrangedSubview(
                AppKitRunningRingView(
                    size: 13,
                    trackColor: palette.borderStrong.nsColor,
                    accentColor: palette.accent.nsColor
                )
            )
        }

        let chev = NSImageView(
            image: LucideVectorCatalog.nsImage(
                for: open ? .chevronDown : .chevronRight,
                size: 16,
                color: (open ? palette.accent : palette.textMuted).nsColor
            )
        )
        chev.translatesAutoresizingMaskIntoConstraints = false
        NSLayoutConstraint.activate([
            chev.widthAnchor.constraint(equalToConstant: 16),
            chev.heightAnchor.constraint(equalToConstant: 16),
        ])
        hStack.addArrangedSubview(chev)
        if !running && !expandsChevronToTrailing {
            hStack.addArrangedSubview(NSView())
        }

        NSLayoutConstraint.activate([
            row.widthAnchor.constraint(equalToConstant: width),
            row.heightAnchor.constraint(equalToConstant: 30),
            hStack.leadingAnchor.constraint(equalTo: row.leadingAnchor, constant: open ? 6 : 0),
            hStack.trailingAnchor.constraint(equalTo: row.trailingAnchor, constant: -8),
            hStack.centerYAnchor.constraint(equalTo: row.centerYAnchor),
        ])
        return row
    }

    private func makeCapsulePill(
        text: String,
        textColor: NSColor,
        fill: NSColor,
        border: NSColor,
        semibold: Bool
    ) -> NSView {
        let box = NSView()
        box.translatesAutoresizingMaskIntoConstraints = false
        box.wantsLayer = true
        box.layer?.cornerRadius = 10
        box.layer?.backgroundColor = fill.cgColor
        box.layer?.borderWidth = 1
        box.layer?.borderColor = border.cgColor

        let lbl = NSTextField(labelWithString: text)
        lbl.font = DesignFonts.nsBodyFont(size: 11, weight: semibold ? .semibold : .regular)
        lbl.textColor = textColor
        lbl.translatesAutoresizingMaskIntoConstraints = false
        box.addSubview(lbl)
        NSLayoutConstraint.activate([
            box.heightAnchor.constraint(equalToConstant: 20),
            lbl.leadingAnchor.constraint(equalTo: box.leadingAnchor, constant: 8),
            lbl.trailingAnchor.constraint(equalTo: box.trailingAnchor, constant: -8),
            lbl.centerYAnchor.constraint(equalTo: box.centerYAnchor),
        ])
        return box
    }

    private func makeLeftRailContainer(
        content: NSView,
        width: CGFloat,
        palette: DesignPalette
    ) -> NSView {
        let wrap = NSView()
        wrap.translatesAutoresizingMaskIntoConstraints = false

        let rail = NSView()
        rail.translatesAutoresizingMaskIntoConstraints = false
        rail.wantsLayer = true
        rail.layer?.backgroundColor = palette.borderStrong.nsColor.cgColor

        content.translatesAutoresizingMaskIntoConstraints = false
        wrap.addSubview(rail)
        wrap.addSubview(content)
        NSLayoutConstraint.activate([
            wrap.widthAnchor.constraint(equalToConstant: width),
            rail.leadingAnchor.constraint(equalTo: wrap.leadingAnchor, constant: 2),
            rail.topAnchor.constraint(equalTo: wrap.topAnchor, constant: 4),
            rail.bottomAnchor.constraint(equalTo: wrap.bottomAnchor, constant: -2),
            rail.widthAnchor.constraint(equalToConstant: 2),
            content.leadingAnchor.constraint(equalTo: wrap.leadingAnchor, constant: 16),
            content.trailingAnchor.constraint(equalTo: wrap.trailingAnchor, constant: -4),
            content.topAnchor.constraint(equalTo: wrap.topAnchor, constant: 4),
            content.bottomAnchor.constraint(equalTo: wrap.bottomAnchor, constant: -4),
        ])
        return wrap
    }

    private func makeExecutionCommandsList(
        commands: [ExecutionCommand],
        width: CGFloat,
        wrappedInSurfaceCard: Bool,
        palette: DesignPalette,
        coordinator: ConversationTableCoordinator
    ) -> NSView {
        let listStack = NSStackView()
        listStack.orientation = .vertical
        listStack.alignment = .leading
        listStack.spacing = 0
        listStack.translatesAutoresizingMaskIntoConstraints = false
        let innerW = wrappedInSurfaceCard ? max(100, width - 20) : width

        for (idx, cmd) in commands.enumerated() {
            let cmdOpen = coordinator.disclosureStore.isOpen(
                scope: coordinator.sessionScopeKey,
                stateKey: cmd.id,
                aliases: cmd.sourceIds,
                initiallyOpen: !cmd.instructionFile && (cmd.status == .running || cmd.status == .failed)
            )
            let itemCol = NSStackView()
            itemCol.orientation = .vertical
            itemCol.alignment = .leading
            itemCol.spacing = 6
            itemCol.translatesAutoresizingMaskIntoConstraints = false
            itemCol.widthAnchor.constraint(equalToConstant: innerW).isActive = true

            let header = HoverSummaryRowView(
                open: false,
                isEnabled: true,
                hoverFill: palette.hover.nsColor.withAlphaComponent(0.55),
                openFill: .clear
            )
            header.onTap = { [weak coordinator] in
                coordinator?.toggleDisclosurePreservingAnchor(stateKey: cmd.id, aliases: cmd.sourceIds, currentlyOpen: cmdOpen)
            }
            let hStack = NSStackView()
            hStack.orientation = .horizontal
            hStack.alignment = .centerY
            hStack.spacing = 8
            hStack.translatesAutoresizingMaskIntoConstraints = false
            header.addSubview(hStack)

            let termIcon = NSImageView(
                image: LucideVectorCatalog.nsImage(for: .terminal, size: 15, color: palette.accent.nsColor)
            )
            termIcon.translatesAutoresizingMaskIntoConstraints = false
            NSLayoutConstraint.activate([
                termIcon.widthAnchor.constraint(equalToConstant: 15),
                termIcon.heightAnchor.constraint(equalToConstant: 15),
            ])
            hStack.addArrangedSubview(termIcon)

            let titleAttr = NSMutableAttributedString()
            if cmd.instructionFile {
                titleAttr.append(NSAttributedString(string: "AGENTS.md", attributes: [
                    .font: DesignFonts.nsBodyFont(size: 12, weight: .medium),
                    .foregroundColor: palette.textPrimary.nsColor,
                ]))
            } else {
                titleAttr.append(NSAttributedString(string: VisibleMarkdownEngine.visibleMarkdown(cmd.title), attributes: [
                    .font: DesignFonts.nsBodyFont(size: 12, weight: .medium),
                    .foregroundColor: palette.textPrimary.nsColor,
                ]))
                if let preview = cmd.preview, !preview.isEmpty {
                    titleAttr.append(NSAttributedString(string: " \(VisibleMarkdownEngine.visibleMarkdown(preview))", attributes: [
                        .font: DesignFonts.nsBodyFont(size: 12),
                        .foregroundColor: palette.textMuted.nsColor,
                    ]))
                }
            }
            let titleLbl = NSTextField(labelWithAttributedString: titleAttr)
            titleLbl.lineBreakMode = .byTruncatingTail
            titleLbl.setContentCompressionResistancePriority(.defaultLow, for: .horizontal)
            hStack.addArrangedSubview(titleLbl)

            hStack.addArrangedSubview(NSView())
            if cmd.status == .running {
                hStack.addArrangedSubview(
                    AppKitRunningRingView(
                        size: 13,
                        trackColor: palette.borderStrong.nsColor,
                        accentColor: palette.accent.nsColor
                    )
                )
            }

            let stLbl = NSTextField(labelWithString: cmd.status.label)
            stLbl.font = DesignFonts.nsBodyFont(size: 12)
            stLbl.textColor = (cmd.status == .failed ? palette.errFg : palette.textMuted).nsColor
            hStack.addArrangedSubview(stLbl)

            let chev = NSImageView(
                image: LucideVectorCatalog.nsImage(
                    for: cmdOpen ? .chevronDown : .chevronRight,
                    size: 16,
                    color: (cmdOpen ? palette.accent : palette.textMuted).nsColor
                )
            )
            chev.translatesAutoresizingMaskIntoConstraints = false
            NSLayoutConstraint.activate([
                chev.widthAnchor.constraint(equalToConstant: 16),
                chev.heightAnchor.constraint(equalToConstant: 16),
                header.widthAnchor.constraint(equalToConstant: innerW),
                header.heightAnchor.constraint(equalToConstant: 30),
                hStack.leadingAnchor.constraint(equalTo: header.leadingAnchor, constant: 4),
                hStack.trailingAnchor.constraint(equalTo: header.trailingAnchor, constant: -4),
                hStack.centerYAnchor.constraint(equalTo: header.centerYAnchor),
            ])
            hStack.addArrangedSubview(chev)
            itemCol.addArrangedSubview(header)

            if cmdOpen {
                let detail = cmd.sections.map { sec in
                    sec.label.isEmpty ? sec.text : "\(sec.label)\n\(sec.text)"
                }.joined(separator: "\n\n")
                itemCol.addArrangedSubview(
                    makeMonospaceDetailBox(
                        text: detail.isEmpty ? "暂无输出" : detail,
                        width: innerW,
                        palette: palette,
                        coordinator: coordinator,
                        linkifyFiles: true
                    )
                )
            }

            if idx < commands.count - 1 {
                let sep = NSView()
                sep.translatesAutoresizingMaskIntoConstraints = false
                sep.wantsLayer = true
                sep.layer?.backgroundColor = palette.borderSubtle.nsColor.cgColor
                NSLayoutConstraint.activate([
                    sep.widthAnchor.constraint(equalToConstant: innerW),
                    sep.heightAnchor.constraint(equalToConstant: 1),
                ])
                itemCol.addArrangedSubview(sep)
            }
            listStack.addArrangedSubview(itemCol)
        }

        if !wrappedInSurfaceCard {
            return listStack
        }
        let card = NSView()
        card.translatesAutoresizingMaskIntoConstraints = false
        card.wantsLayer = true
        card.layer?.cornerRadius = 10
        card.layer?.backgroundColor = palette.surface.nsColor.cgColor
        card.layer?.borderWidth = 1
        card.layer?.borderColor = palette.borderSubtle.nsColor.cgColor
        card.addSubview(listStack)
        NSLayoutConstraint.activate([
            card.widthAnchor.constraint(equalToConstant: width),
            listStack.leadingAnchor.constraint(equalTo: card.leadingAnchor, constant: 10),
            listStack.trailingAnchor.constraint(equalTo: card.trailingAnchor, constant: -10),
            listStack.topAnchor.constraint(equalTo: card.topAnchor, constant: 2),
            listStack.bottomAnchor.constraint(equalTo: card.bottomAnchor, constant: -2),
        ])
        return card
    }

    private func makeMonospaceDetailBox(
        text: String,
        width: CGFloat,
        palette: DesignPalette,
        coordinator: ConversationTableCoordinator,
        linkifyFiles: Bool
    ) -> NSView {
        let cleaned = VisibleMarkdownEngine.visibleMarkdown(text)
        let pStyle = NSMutableParagraphStyle()
        pStyle.minimumLineHeight = 19.2 // 12pt * 1.6
        pStyle.maximumLineHeight = 19.2
        let baseAttrs: [NSAttributedString.Key: Any] = [
            .font: DesignFonts.nsMonoFont(size: 12),
            .foregroundColor: palette.textSecondary.nsColor,
            .paragraphStyle: pStyle,
        ]
        let attr = NSMutableAttributedString()
        if linkifyFiles {
            let spans = FileLinksAndPlanExport.fileLinkSpans(cleaned)
            let ns = cleaned as NSString
            var cursor = 0
            for span in spans where span.nsRange.location >= cursor {
                if span.nsRange.location > cursor {
                    let prefix = ns.substring(with: NSRange(location: cursor, length: span.nsRange.location - cursor))
                    attr.append(NSAttributedString(string: prefix, attributes: baseAttrs))
                }
                var linkAttrs = baseAttrs
                linkAttrs[.foregroundColor] = palette.accent.nsColor
                linkAttrs[.underlineStyle] = NSUnderlineStyle.single.rawValue
                linkAttrs[.link] = span.href
                attr.append(NSAttributedString(string: span.label, attributes: linkAttrs))
                cursor = span.nsRange.location + span.nsRange.length
            }
            if cursor < ns.length {
                attr.append(NSAttributedString(string: ns.substring(from: cursor), attributes: baseAttrs))
            }
        } else {
            attr.append(NSAttributedString(string: cleaned, attributes: baseAttrs))
        }

        let innerW = max(80, width - 26)
        let contentH = max(22, ReaderSelectableTextView.measureHeight(attributed: attr, width: innerW))
        let boxH = min(240, max(48, contentH + 22))

        let scroll = PassthroughInnerScrollView(allowsVerticalScroll: true)
        scroll.hasVerticalScroller = contentH + 22 > boxH
        scroll.autohidesScrollers = true
        scroll.drawsBackground = true
        scroll.backgroundColor = palette.codeBg.nsColor
        scroll.wantsLayer = true
        scroll.layer?.cornerRadius = 10
        scroll.layer?.borderWidth = 1
        scroll.layer?.borderColor = palette.borderSubtle.nsColor.cgColor

        let tv = ReaderSelectableTextView(
            frame: NSRect(x: 0, y: 0, width: max(100, width), height: max(boxH, contentH + 22))
        )
        tv.textContainerInset = NSSize(width: 13, height: 11)
        tv.applyAttributedText(attr, width: max(100, width))
        tv.onLinkActivated = { [weak self, weak coordinator] href in
            guard let self, let coordinator else { return false }
            return self.handleActivatedHref(href, coordinator: coordinator)
        }
        scroll.documentView = tv
        scroll.translatesAutoresizingMaskIntoConstraints = false
        NSLayoutConstraint.activate([
            scroll.widthAnchor.constraint(equalToConstant: max(100, width)),
            scroll.heightAnchor.constraint(equalToConstant: boxH),
        ])
        return scroll
    }
}
