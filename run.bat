@echo off
REM 在 wa2-upscale 目录双击或 cmd 里运行。云电脑单卡 NVIDIA 用设备 0。
REM --workers 是并行 waifu2x 进程数，不是图片批次数；12GB 显存建议保持 3。
REM   run.bat
REM   run.bat --match tv100100.tga --limit 1
cd /d "%~dp0"
python upscale_art.py --gpu 0 --workers 3 --jobs 4:8:4 --batch 8 --data .\data --bin .\waifu2x-ncnn-vulkan\waifu2x-ncnn-vulkan.exe %*
