@echo off
REM 火山引擎 ImageX 云端超分；run.bat 仍保留本地 waifu2x 流程。
cd /d "%~dp0"
python imagex_upscale.py --data .\data --multiple 8 %*
