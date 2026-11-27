using System.Runtime.CompilerServices;

namespace LeseLang.Operations;

/// <summary>One handler invocation. Use Commit for synchronous mutations after awaited work.</summary>
public sealed class OperationContext
{
    private readonly Func<bool> allows;
    private readonly object gate = new();
    private bool closed;
    private bool checking;
    private bool committing;
    public CancellationToken CancellationToken { get; }

    internal OperationContext(CancellationToken cancellation, Func<bool> allows)
    {
        CancellationToken = cancellation;
        this.allows = allows;
    }

    internal void CheckAuthority()
    {
        lock (gate)
        {
            if (closed || checking) throw new InvalidOperationException("Operation context unavailable.");
            checking = true;
            try
            {
                CancellationToken.ThrowIfCancellationRequested();
                if (!allows()) throw new UnauthorizedAccessException("Operation grant revoked.");
                CancellationToken.ThrowIfCancellationRequested();
            }
            catch { closed = true; throw; }
            finally { checking = false; }
        }
    }

    /// <summary>Checks the current grant and executes one synchronous application mutation.</summary>
    public void Commit(Action mutation)
    {
        ArgumentNullException.ThrowIfNull(mutation);
        if (mutation.GetInvocationList().Length != 1
            || mutation.Method.IsDefined(typeof(AsyncStateMachineAttribute), false))
            throw new ArgumentException("Commit requires one synchronous mutation.", nameof(mutation));
        lock (gate)
        {
            if (committing) throw new InvalidOperationException("Nested operation commit.");
            CheckAuthority();
            committing = true;
            try { mutation(); }
            finally { committing = false; }
        }
    }

    internal void Close()
    {
        lock (gate) { closed = true; }
    }

    internal void Complete()
    {
        lock (gate)
        {
            CheckAuthority();
            closed = true;
        }
    }
}
