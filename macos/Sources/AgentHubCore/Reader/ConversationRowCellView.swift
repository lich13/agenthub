import AppKit
import SwiftUI

// MARK: - AppKit NSTableView Coordinator with Deterministic 0.0pt Scroll Anchor Lock

@MainActor
final class ConversationRowCellView: NSTableCellView {
    let containerStack = NSStackView()
    private(set) var floatingSideCopyButton: IconFeedbackActionButton?
    private(set) var questionReplyCopyButtons: [IconFeedbackActionButton] = []
    private var hoverTrackingArea: NSTrackingArea?
    private(set) var isRowHovered: Bool = false {
        didSet { updateFloatingCopyVisibility() }
    }
    private var configuredLeadingInset: CGFloat = 0
    private var configuredContentWidth: CGFloat = 0
    private var leadingConstraint: NSLayoutConstraint?
    private var stackConstraints: [NSLayoutConstraint] = []

    private struct CellConfigSignature: Equatable {
        let sessionScopeKey: String
        let item: ConversationRowItem
        let contentWidth: CGFloat
        let highlighted: Bool
        let scheme: ColorSchemeKind
        let workspace: WorkspaceId
        let disclosureRev: UInt64
        let scrollerStyle: NSScroller.Style
        let loadedAttachmentState: ConversationTableCoordinator.LoadedAttachmentState
    }

    private var lastConfiguredSignature: CellConfigSignature?
    var cachedRowId: String?

    init(identifier: NSUserInterfaceItemIdentifier) {
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
    static func floatingSideCopyButtonX(
        leadingInset: CGFloat,
        contentWidth: CGFloat,
        cellWidth: CGFloat
    ) -> CGFloat {
        let desired = leadingInset + contentWidth + 6
        return max(8, min(max(8, cellWidth - 34), desired))
    }

    override func updateTrackingAreas() {
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

    override func mouseEntered(with event: NSEvent) {
        isRowHovered = true
    }

    override func mouseExited(with event: NSEvent) {
        isRowHovered = false
    }

    override func layout() {
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

    func setSimulatedHoverForTesting(_ hovered: Bool) {
        isRowHovered = hovered
    }

    func updateFloatingCopyVisibility() {
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

    func updateHorizontalInset(_ leadingInset: CGFloat) {
        configuredLeadingInset = leadingInset
        if let leadingConstraint, abs(leadingConstraint.constant - leadingInset) > 0.01 {
            leadingConstraint.constant = leadingInset
        }
        positionFloatingCopyButton()
    }

    func configure(
        item: ConversationRowItem,
        leadingInset: CGFloat,
        trailingInset _: CGFloat,
        contentWidth: CGFloat,
        highlighted: Bool,
        coordinator: ConversationTableCoordinator
    ) {
        let nextSig = CellConfigSignature(
            sessionScopeKey: coordinator.sessionScopeKey,
            item: item,
            contentWidth: contentWidth,
            highlighted: highlighted,
            scheme: coordinator.scheme,
            workspace: coordinator.workspace,
            disclosureRev: coordinator.disclosureStore.revision,
            scrollerStyle: ConversationTableCoordinator.effectiveScrollerStyle,
            loadedAttachmentState: coordinator.loadedAttachmentState(for: item)
        )
        if lastConfiguredSignature == nextSig, !containerStack.arrangedSubviews.isEmpty {
            updateHorizontalInset(leadingInset)
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
        let lead = containerStack.leadingAnchor.constraint(equalTo: leadingAnchor, constant: leadingInset)
        leadingConstraint = lead
        stackConstraints = [
            lead,
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
            buildMarkdownViews(
                ownerId: item.id,
                markdown: markdown,
                foldInstructions: foldInstructions,
                width: contentWidth,
                coordinator: coordinator,
                alreadyCleaned: true
            )
            let cleanedReply = markdown
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
        case let .claudeProcessGroup(proc):
            buildClaudeProcessGroupView(proc: proc, width: contentWidth, coordinator: coordinator)
        case let .claudeMetadataGroup(meta):
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
        let (proj, segs) = UserContextEngine.projectAndSegmentUserMessage(body, contextBlocks: msg?.contextBlocks)
        let identity = msg?.id ?? item.timelineId

        let rightAlignRow = NSStackView()
        rightAlignRow.orientation = .vertical
        rightAlignRow.alignment = .trailing
        rightAlignRow.spacing = 10
        rightAlignRow.translatesAutoresizingMaskIntoConstraints = false
        rightAlignRow.widthAnchor.constraint(equalToConstant: width).isActive = true

        if let attachments = msg?.attachments, !attachments.isEmpty {
            let wrappedRows = ConversationTableCoordinator.layoutUserAttachmentRows(
                attachments: attachments,
                namesOnly: namesOnly,
                maxWidth: width
            )
            let attGroupCol = NSStackView()
            attGroupCol.orientation = .vertical
            attGroupCol.alignment = .trailing
            attGroupCol.spacing = 8
            attGroupCol.translatesAutoresizingMaskIntoConstraints = false

            for rowItems in wrappedRows {
                let attRow = NSStackView()
                attRow.orientation = .horizontal
                attRow.alignment = .centerY
                attRow.spacing = 8
                for entry in rowItems {
                    let att = entry.attachment
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
                            let failed = (att.reason != nil) || coordinator.attachmentViewport.hasFailed(for: key)
                            thumbBtn.image = LucideVectorCatalog.nsImage(
                                for: .imageOff,
                                size: 22,
                                color: palette.textMuted.nsColor
                            )
                            thumbBtn.imagePosition = .imageAbove
                            thumbBtn.font = DesignFonts.nsBodyFont(size: 11)
                            thumbBtn.contentTintColor = palette.textMuted.nsColor
                            thumbBtn.title = failed ? "图片不可用" : "图片"
                            thumbBtn.toolTip = att.reason ?? (failed ? "图片不可用" : att.name)
                            thumbBtn.isEnabled = false
                        }
                        attRow.addArrangedSubview(thumbBtn)
                    } else {
                        attRow.addArrangedSubview(
                            makeFileAttachmentCard(
                                att: att,
                                namesOnly: namesOnly,
                                size: entry.size,
                                palette: palette,
                                coordinator: coordinator
                            )
                        )
                    }
                }
                attGroupCol.addArrangedSubview(attRow)
            }
            rightAlignRow.addArrangedSubview(attGroupCol)
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
                let singleLineW: CGFloat = max(36, ceil(attr.size().width) + 4)
                let maxTextW: CGFloat = maxBubbleW - (hPad * 2)
                let textW: CGFloat = min(maxTextW, singleLineW)
                let textH: CGFloat = max(24, ReaderSelectableTextView.measureHeight(attributed: attr, width: textW))
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
                    toolTipText: "复制答复"
                ) { [weak self, weak coordinator] in
                    NSPasteboard.general.clearContents()
                    NSPasteboard.general.setString(a, forType: .string)
                    coordinator?.onCopyFeedback?("已复制答复")
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
                    qHeaderBox.hoverFill = palette.hover.nsColor
                    qHeaderBox.focusRingColor = palette.accent.nsColor
                    qHeaderBox.accessibilityExpandedState = qOpen
                    qHeaderBox.setAccessibilityLabel(trimmedQ)
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
        bubble.layer?.backgroundColor = palette.userBubble.nsColor.cgColor
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
        header.hoverFill = palette.hover.nsColor
        header.focusRingColor = palette.accent.nsColor
        header.accessibilityExpandedState = open
        header.setAccessibilityLabel("上下文")
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
        if open {
            hStack.addArrangedSubview(NSView())
        }
        hStack.addArrangedSubview(titleLbl)
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
        size: NSSize,
        palette: DesignPalette,
        coordinator: ConversationTableCoordinator
    ) -> NSView {
        let card = NSStackView()
        card.orientation = .horizontal
        card.alignment = .centerY
        card.spacing = 8
        card.translatesAutoresizingMaskIntoConstraints = false
        card.wantsLayer = true
        card.layer?.cornerRadius = 12
        card.layer?.backgroundColor = palette.surface.nsColor.cgColor
        card.layer?.borderWidth = 1
        card.layer?.borderColor = palette.borderStrong.nsColor.cgColor
        card.edgeInsets = NSEdgeInsets(top: 8, left: 12, bottom: 8, right: 10)
        NSLayoutConstraint.activate([
            card.widthAnchor.constraint(equalToConstant: size.width),
            card.heightAnchor.constraint(equalToConstant: size.height),
        ])

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
        textCol.setContentCompressionResistancePriority(.defaultLow, for: .horizontal)

        if !namesOnly, let rawPath = att.path, !rawPath.isEmpty {
            let linkBtn = ActionButton()
            linkBtn.lineBreakMode = .byTruncatingTail
            linkBtn.setContentCompressionResistancePriority(.defaultLow, for: .horizontal)
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
            nameLbl.lineBreakMode = .byTruncatingTail
            nameLbl.setContentCompressionResistancePriority(.defaultLow, for: .horizontal)
            textCol.addArrangedSubview(nameLbl)
        }

        if let reason = att.reason, !reason.isEmpty {
            let reasonLbl = NSTextField(labelWithString: reason)
            reasonLbl.font = DesignFonts.nsBodyFont(size: 11)
            reasonLbl.textColor = palette.textMuted.nsColor
            reasonLbl.lineBreakMode = .byTruncatingTail
            reasonLbl.setContentCompressionResistancePriority(.defaultLow, for: .horizontal)
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

    private func buildPlanView(
        id _: String,
        markdown: String,
        statusText: String,
        fallbackTitle: String,
        width: CGFloat,
        coordinator: ConversationTableCoordinator
    ) {
        let palette = coordinator.palette
        let cleanedPlan = markdown

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
                do {
                    let _: PlanSaveResult = try await coordinator.store.rpc(
                        "plans.save",
                        args: RPCRequestEnvelope(request: PlanSaveRequest(filename: filename, markdown: cleanedPlan)),
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
            transport: coordinator.store.transport,
            alreadyCleaned: true
        )
        for seg in segs {
            switch seg {
            case let .prose(attr):
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

    static func subagentActivityVerb(_ sub: SubagentActivity) -> String {
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
            openFill: .clear,
            focusRingColor: palette.accent.nsColor,
            accessibilityExpanded: false,
            accessibilityLabelText: sub.name
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
        nameLbl.textColor = palette.textMuted.nsColor
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
}
