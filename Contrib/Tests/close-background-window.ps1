param([Parameter(Mandatory=$true)][long]$WindowHandle, [Parameter(Mandatory=$true)][int]$OwnerProcessId)
$ErrorActionPreference = 'Stop'
Add-Type -AssemblyName UIAutomationClient
Add-Type -AssemblyName UIAutomationTypes
Add-Type @'
using System;
using System.Runtime.InteropServices;
public static class OwnedWindowInput {
    [StructLayout(LayoutKind.Sequential)] public struct Point { public int X, Y; }
    [StructLayout(LayoutKind.Sequential)] public struct Rect { public int Left, Top, Right, Bottom; }
    [DllImport("user32.dll")] public static extern IntPtr SetThreadDpiAwarenessContext(IntPtr value);
    [DllImport("user32.dll")] public static extern uint GetWindowThreadProcessId(IntPtr window, out uint processId);
    [DllImport("user32.dll")] public static extern bool ScreenToClient(IntPtr window, ref Point point);
    [DllImport("user32.dll")] public static extern bool GetClientRect(IntPtr window, out Rect rect);
    [DllImport("user32.dll")] public static extern bool PostMessage(IntPtr window, uint message, IntPtr wParam, IntPtr lParam);
}
'@
[void][OwnedWindowInput]::SetThreadDpiAwarenessContext([IntPtr](-4))
$window = [System.Windows.Automation.AutomationElement]::FromHandle([IntPtr]$WindowHandle)
if ($window.Current.ProcessId -ne $OwnerProcessId) { throw 'Window ownership changed' }
function NamedElement([string]$name) {
    $condition = New-Object System.Windows.Automation.PropertyCondition([System.Windows.Automation.AutomationElement]::NameProperty, $name)
    return $window.FindFirst([System.Windows.Automation.TreeScope]::Descendants, $condition)
}
function AwaitElement([string]$name) {
    $deadline = [DateTime]::UtcNow.AddSeconds(15)
    do {
        $element = NamedElement $name
        if ($null -ne $element) { return $element }
        Start-Sleep -Milliseconds 100
    } while ([DateTime]::UtcNow -lt $deadline)
    throw "Desktop control was unavailable: $name"
}
function InvokeElement($element) {
    if ($element.Current.IsOffscreen -or -not $element.Current.IsEnabled) { throw 'Desktop control is not interactive' }
    $pattern = $null
    if ($element.TryGetCurrentPattern([System.Windows.Automation.InvokePattern]::Pattern, [ref]$pattern)) {
        $pattern.Invoke()
        return
    }
    # Navigation items handle pointer presses without exposing an InvokePattern.
    # Deliver a normal click to the test-owned HWND without moving the user's pointer.
    [uint32]$owner = 0
    [void][OwnedWindowInput]::GetWindowThreadProcessId([IntPtr]$WindowHandle, [ref]$owner)
    if ($owner -ne $OwnerProcessId) { throw 'Window ownership changed' }
    $bounds = $element.Current.BoundingRectangle
    $point = New-Object OwnedWindowInput+Point
    $point.X = [int]($bounds.Left + $bounds.Width / 2)
    $point.Y = [int]($bounds.Top + $bounds.Height / 2)
    $rect = New-Object OwnedWindowInput+Rect
    if (-not [OwnedWindowInput]::ScreenToClient([IntPtr]$WindowHandle, [ref]$point) -or
        -not [OwnedWindowInput]::GetClientRect([IntPtr]$WindowHandle, [ref]$rect) -or
        $point.X -lt 0 -or $point.Y -lt 0 -or $point.X -ge $rect.Right -or $point.Y -ge $rect.Bottom) { throw 'Desktop control is outside the owned window' }
    $position = [IntPtr](($point.Y -shl 16) -bor $point.X)
    foreach ($event in @(@(0x0200, 0), @(0x0201, 1), @(0x0202, 0))) {
        if (-not [OwnedWindowInput]::PostMessage([IntPtr]$WindowHandle, $event[0], [IntPtr]$event[1], $position)) { throw 'Could not click the owned desktop control' }
    }
}
InvokeElement (AwaitElement 'Settings')
$label = AwaitElement 'Run in background when window closed'
$rowY = $label.Current.BoundingRectangle.Top + $label.Current.BoundingRectangle.Height / 2
$controls = $window.FindAll([System.Windows.Automation.TreeScope]::Descendants, [System.Windows.Automation.Condition]::TrueCondition)
$candidates = @()
foreach ($control in $controls) {
    $pattern = $null
    if ($control.TryGetCurrentPattern([System.Windows.Automation.TogglePattern]::Pattern, [ref]$pattern)) {
        $bounds = $control.Current.BoundingRectangle
        if (-not $control.Current.IsOffscreen -and [Math]::Abs(($bounds.Top + $bounds.Height / 2) - $rowY) -lt 15) { $candidates += $pattern }
    }
}
if ($candidates.Count -ne 1) { throw "Expected one background toggle in its labeled row; found $($candidates.Count)" }
if ($candidates[0].Current.ToggleState -ne [System.Windows.Automation.ToggleState]::On) { throw 'Background setting was not enabled' }
$candidates[0].Toggle()
InvokeElement (AwaitElement 'Done')
Write-Output 'Disabled hide-on-close through the owned desktop settings window.'
