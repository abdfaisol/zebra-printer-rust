@echo off
echo ===================================================
echo   Print Wareify (Rust + Tauri + Tailwind CSS)
echo ===================================================
cd /d "%~dp0"

echo Building Frontend (Vite + Tailwind)...
call npm run build

echo.
echo Building Rust Desktop App & Installer (MSI + EXE)...
call npm run tauri build

echo.
echo ===================================================
echo Build selesai! Hasil ada di:
echo src-tauri\target\release\bundle\msi\
echo src-tauri\target\release\bundle\nsis\
echo ===================================================
pause
