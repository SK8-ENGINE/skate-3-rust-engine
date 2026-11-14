# Custom Music helper (ADD-MY-MUSIC.bat runs this): pick a music folder, pack it into a music bank in
# Documents\Skate Music (outside the mod, so sharing the mod never shares music), then rebuild the mod's playlist
# from every bank there. A folder dragged onto the .bat is used instead of the picker.
param([string]$MusicFolder = '')
$ErrorActionPreference = 'Stop'

function Say([string]$Text) { Write-Host ''; Write-Host $Text }

# The game sits next to the mods folder; in a source checkout it is in bin\.
$exe = [IO.Path]::GetFullPath((Join-Path $PSScriptRoot '..\..\skate3rust.exe'))
$dev = [IO.Path]::GetFullPath((Join-Path $PSScriptRoot '..\..\bin\skate3rust.exe'))
if (-not (Test-Path -LiteralPath $exe) -and (Test-Path -LiteralPath $dev)) { $exe = $dev }
if (-not (Test-Path -LiteralPath $exe)) {
    Say "Could not find the game (skate3rust.exe) two folders up from this mod: $exe"
    Say 'This mod must stay in the game''s "mods" folder. Move it back there and try again.'
    exit 1
}

if (-not $MusicFolder) {
    Add-Type -AssemblyName System.Windows.Forms
    $dialog = New-Object System.Windows.Forms.FolderBrowserDialog
    $dialog.Description = 'Pick the folder that holds your music (MP3, FLAC, M4A, OGG, WAV ...)'
    $dialog.ShowNewFolderButton = $false
    if ($dialog.ShowDialog() -ne [System.Windows.Forms.DialogResult]::OK) { Say 'No folder picked. Nothing was changed.'; exit 0 }
    $MusicFolder = $dialog.SelectedPath
}
if (-not (Test-Path -LiteralPath $MusicFolder -PathType Container)) { Say "That is not a folder: $MusicFolder"; exit 1 }

$banks = Join-Path ([Environment]::GetFolderPath('MyDocuments')) 'Skate Music'
New-Item -ItemType Directory -Force -Path $banks | Out-Null
$name = (Split-Path -Leaf $MusicFolder) -replace '[\\/:*?"<>|]', '_'
$bank = Join-Path $banks "$name.bank"

Say "Packing the music in: $MusicFolder"
Say "Into: $bank"
Say 'This can take a few minutes for a big folder. Each song is listed as it is added.'
& $exe --pack-music-folder $MusicFolder $bank
if ($LASTEXITCODE -ne 0) { Say 'Packing did not work (see the lines above). Nothing else was changed.'; exit 1 }

$all = @(Get-ChildItem -LiteralPath $banks -Filter '*.bank' -File | ForEach-Object { $_.FullName })
$playlists = Join-Path $PSScriptRoot 'playlists'
New-Item -ItemType Directory -Force -Path $playlists | Out-Null
$playlist = Join-Path $playlists 'my-music.json'
& $exe --music-playlist $playlist 'My music' --shuffle @all
if ($LASTEXITCODE -ne 0) { Say 'Making the playlist did not work (see the lines above).'; exit 1 }

Say "Done. Your playlist has the music of $($all.Count) folder(s) from $banks."
Say 'Now start the game, open the mod menu and turn on Custom Music.'
