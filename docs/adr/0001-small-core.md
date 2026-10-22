# Keep the launcher core small and put AI in extensions

The product is a general-purpose desktop launcher for Windows, macOS and Linux, with a small permanent feature core and low resource usage. Users can author and install extensions; AI is extension functionality rather than a mandatory part of the core. This makes the launcher useful independently of AI services and prevents optional workflows from continually expanding the permanent product surface.

The initial default features are application launching, calculator, quicklinks, file search and optional clipboard history. Each must be individually disableable. Shipping a feature by default does not by itself require putting its implementation in the core.

The host/extension API, runtime and resource budgets remain open. Trust and distribution were subsequently settled in [ADR 0002](0002-trusted-extensions-and-open-distribution.md), UI and GPUI CE renderer in [ADR 0003](0003-gpui-ce-and-extensible-views.md), with accepted disable/data behavior in [extension policies](../extension-policy-proposal.md). Platform behavior should be as consistent as practical, with explicit differences where OS capabilities require them.
