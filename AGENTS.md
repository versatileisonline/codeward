# Codeward workflow

Codeward is a native desktop application. Use the native app for previews,
interaction checks, and visual QA. Do not start localhost browser previews or
use browser automation for product validation unless the user explicitly asks.

Frontend unit tests and asset builds are part of the desktop development workflow.
After frontend assets change, rebuild the native executable and update the local
app bundle before presenting the result.
