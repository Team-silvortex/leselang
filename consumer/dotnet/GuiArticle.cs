using System.Text.Json;
using static GuiWire;

internal enum GuiFault
{
    None, WrongPublishTarget, UnversionedEdit, RejectClick, UnacknowledgedClick, AcknowledgeWithoutEdit,
    FinalReadInvalid, PublicationSuperseded, DisabledDraft, ReadonlyDraft, CancelBeforeEdit, CancelAfterEdit,
    ReplacedDraftAtWrite, ExternalEditAtWrite, WriteGrantRevoked
}

// This is the C# application's state. Each worker session gets a new binding to the same article.
internal sealed class GuiArticle(GuiFault fault, string supersedingText, Action cancelBinding)
{
    private sealed class DraftControl(int generation)
    {
        internal int Generation { get; } = generation;
        internal int Revision { get; private set; } = 30;
        internal string Value { get; private set; } = "";
        internal void SetText(string text)
        {
            Value = text;
            Revision++;
        }
    }

    private DraftControl draftControl = new(7);
    private bool writeGranted = true;
    internal string Draft => draftControl.Value;
    internal string Published { get; private set; } = "";
    internal int Edits { get; private set; }
    internal int Clicks { get; private set; }
    internal List<string> Trace { get; } = [];
    private int publishRevision = 80;

    private int Generation(string name) => name switch
    {
        "draft" => draftControl.Generation,
        "publish" => 13,
        _ => throw new InvalidDataException($"Unknown control {name}")
    };

    private object Snapshot(string name, GuiFault active)
    {
        bool draft = name == "draft";
        int revision = draft ? draftControl.Revision : publishRevision;
        string identifier = name;
        if (draft && Edits > 0 && active == GuiFault.UnversionedEdit) revision = 30;
        // The publication program has already observed the click before these final host reads.
        if (draft && Clicks == 1 && active == GuiFault.FinalReadInvalid) identifier = "foreign";
        if (draft && Clicks == 1 && active == GuiFault.PublicationSuperseded)
        {
            Published = supersedingText;
            publishRevision++;
        }
        return new { identifier, generation = Generation(name), role = draft ? "text_field" : "button",
            title = name, value = draft ? Draft : Published, enabled = !draft || fault != GuiFault.DisabledDraft,
            editable = draft && fault != GuiFault.ReadonlyDraft, revision, clicks = draft ? 0 : Clicks };
    }

    internal GuiWorker.Reply Dispatch(string operation, JsonElement arguments) => Handle(operation, arguments, readOnly: false);
    internal GuiWorker.Reply Read(string operation, JsonElement arguments) => Handle(operation, arguments, readOnly: true);

    private GuiWorker.Reply Handle(string operation, JsonElement args, bool readOnly)
    {
        string target = operation switch
        {
            "gui.list" => "",
            "gui.find" => Text(args, "name"),
            "gui.observe" or "gui.set_text" or "gui.click" => Text(args.GetProperty("target"), "name"),
            _ => throw new InvalidDataException($"Unknown GUI operation {operation}")
        };
        if (!readOnly) Trace.Add(operation[4..] + (target.Length == 0 ? "" : ":" + target));
        GuiFault active = readOnly ? GuiFault.None : fault;
        switch (operation)
        {
            case "gui.list":
                Fields(args);
                return new(new[] { "draft", "publish" });
            case "gui.find":
                Fields(args, "name");
                if (active == GuiFault.WrongPublishTarget && Edits > 0 && target == "publish") target = "draft";
                return new(new { name = target, generation = Generation(target) });
        }

        var handle = args.GetProperty("target");
        Fields(handle, "name", "generation");
        if (readOnly && operation != "gui.observe") throw new InvalidDataException("Readback attempted a write");
        if (operation == "gui.set_text" && target == "draft")
        {
            switch (active)
            {
                case GuiFault.ReplacedDraftAtWrite: draftControl = new(draftControl.Generation + 1); break;
                case GuiFault.ExternalEditAtWrite: draftControl.SetText(supersedingText); break;
                case GuiFault.WriteGrantRevoked: writeGranted = false; break;
            }
        }
        if (Number(handle, "generation") != Generation(target)) return new(null, "stale_generation");
        if (operation == "gui.observe")
        {
            Fields(args, "target");
            return new(Snapshot(target, active));
        }
        if (!writeGranted) return new(null, "write_denied");
        if (operation == "gui.set_text")
        {
            Fields(args, "target", "expected_revision", "text");
            if (target != "draft" || fault is GuiFault.ReadonlyDraft or GuiFault.DisabledDraft)
                throw new InvalidDataException("Unexpected draft write");
            if (Number(args, "expected_revision") != draftControl.Revision) return new(null, "stale_revision");
            string text = Text(args, "text");
            if (active == GuiFault.CancelBeforeEdit)
            {
                cancelBinding();
                return new(false);
            }
            if (active != GuiFault.AcknowledgeWithoutEdit)
            {
                draftControl.SetText(text);
                Edits++;
            }
            if (active == GuiFault.CancelAfterEdit) cancelBinding();
            return new(true);
        }

        Fields(args, "target", "expected_revision");
        if (target != "publish" || Number(args, "expected_revision") != publishRevision)
            throw new InvalidDataException("Unexpected publication click");
        if (active == GuiFault.RejectClick) return new(null, "cannot_publish");
        Published = Draft;
        Clicks++;
        publishRevision++;
        return new(active != GuiFault.UnacknowledgedClick);
    }
}
