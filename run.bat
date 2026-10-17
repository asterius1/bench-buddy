@echo off
rem Bench Buddy checker: watches the golem log folder, page on http://127.0.0.1:8771/
rem Code changes are picked up by themselves; a change to core\serve.py restarts it here (exit code 3).
cd /d "%~dp0"
python core\serve.py %*
:again
if not "%errorlevel%"=="3" goto :eof
python core\serve.py --no-browser %*
goto again
