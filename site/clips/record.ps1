<#
.SYNOPSIS
Records the clips of the tutorial again and makes the files of docs/clips and the pictures at the
top of the home page.

.DESCRIPTION
genie runs without a window and saves each frame as a PNG file, in the light and the dark look.
ffmpeg then makes a looping MP4 file and a WebP poster for each clip. The clips use real data
from the web: hg38 from UCSC, ClinVar, phyloP and the exome reads of NA12878. The work folder
keeps the fetched ranges, so later runs fetch only new ranges, and the cap stops a run that would
take the bytes of all runs over it. A first run of all clips fetches about 35 MB.

Needs ffmpeg with libx264 and libwebp.

.EXAMPLE
site/clips/record.ps1

.EXAMPLE
site/clips/record.ps1 -Clips cards, track-menu -Cap 60MB

.EXAMPLE
site/clips/record.ps1 -Clips cards -SkipRecord -Crf 24
Encodes the frames of an earlier run again, without a new recording.
#>
param(
    [string[]]$Clips = @(
        'open-genome', 'go-to-gene', 'several-loci', 'add-tracks', 'move-zoom', 'open-url',
        'cards', 'track-menu', 'find-sequence', 'save-export', 'hero'
    ),
    [long]$Cap = 50MB,
    [string]$Work = '',
    [int]$Crf = 20,
    [switch]$SkipRecord
)
$ErrorActionPreference = 'Stop'

# Runs a program. Its messages on the error output do not stop the script, but an exit code that
# is not 0 does.
function Invoke-Program([string]$What, [scriptblock]$Program) {
    $ErrorActionPreference = 'Continue'
    & $Program
    if ($LASTEXITCODE -ne 0) {
        throw "$What stopped with the exit code $LASTEXITCODE."
    }
}

# With -File, a list such as "cards,hero" comes as one text.
$Clips = @($Clips | ForEach-Object { $_ -split ',' } | ForEach-Object { $_.Trim() } | Where-Object { $_ })
$repository = (Resolve-Path (Join-Path $PSScriptRoot '..\..')).Path
if (-not $Work) {
    $Work = Join-Path $repository 'target\clips'
}
if (-not (Get-Command ffmpeg -ErrorAction SilentlyContinue)) {
    throw 'ffmpeg is not on the PATH.'
}
$folders = @{}
foreach ($name in 'cache', 'frames', 'config', 'data') {
    $folders[$name] = Join-Path $Work $name
    New-Item -ItemType Directory -Force $folders[$name] | Out-Null
}

if (-not $SkipRecord) {
    Push-Location $repository
    try {
        $artifacts = Invoke-Program 'cargo' {
            cargo test -p genie --bin genie --locked --no-run --message-format json
        } | ForEach-Object { try { $_ | ConvertFrom-Json } catch { $null } }
    } finally {
        Pop-Location
    }
    $binary = $artifacts |
        Where-Object { $_ -and $_.reason -eq 'compiler-artifact' -and $_.executable -and $_.target.name -eq 'genie' } |
        Select-Object -Last 1
    if (-not $binary) {
        throw 'cargo did not build the tests of genie.'
    }
    # The run gets its own settings and cache folders, so it does not change those of the user.
    # The variables go back to their values at the end.
    $variables = [ordered]@{
        GENIE_NET_CACHE   = $folders['cache']
        GENIE_NET_CAP     = "$Cap"
        GENIE_NET_LOG     = Join-Path $Work 'fetch.log'
        GENIE_CLIP_FRAMES = $folders['frames']
        ICED_TEST_BACKEND = 'tiny-skia'
        APPDATA           = $folders['config']
        LOCALAPPDATA      = $folders['data']
        XDG_CONFIG_HOME   = $folders['config']
        XDG_CACHE_HOME    = $folders['data']
        HOME              = $folders['data']
    }
    $saved = @{}
    foreach ($name in $variables.Keys) {
        $saved[$name] = [Environment]::GetEnvironmentVariable($name)
    }
    try {
        foreach ($name in $variables.Keys) {
            [Environment]::SetEnvironmentVariable($name, $variables[$name])
        }
        foreach ($clip in $Clips) {
            $test = 'app::clips::' + ($clip -replace '-', '_')
            Invoke-Program "The clip $clip" {
                & $binary.executable $test --exact --ignored --nocapture --test-threads=1
            }
        }
    } finally {
        foreach ($name in $variables.Keys) {
            [Environment]::SetEnvironmentVariable($name, $saved[$name])
        }
    }
}

$videos = Join-Path $repository 'docs\clips'
$images = Join-Path $repository 'docs\images'
foreach ($clip in $Clips) {
    foreach ($look in 'light', 'dark') {
        $frames = Join-Path $folders['frames'] "$clip-$look"
        $last = Get-ChildItem $frames -Filter 'f*.png' -ErrorAction SilentlyContinue |
            Sort-Object Name | Select-Object -Last 1
        if (-not $last) {
            throw "The clip $clip-$look has no frames in $frames."
        }
        if ($clip -eq 'hero') {
            $picture = Join-Path $images "hero-$look.webp"
            Invoke-Program 'ffmpeg' {
                ffmpeg -hide_banner -loglevel error -y -i $last.FullName -vf 'crop=2048:500:0:0' -c:v libwebp -quality 88 $picture
            }
            '{0,-20} {1,6:N0} KB picture' -f "hero-$look", ((Get-Item $picture).Length / 1KB)
            continue
        }
        $pattern = Join-Path $frames ($last.Name -replace '^f\d{5}', 'f%05d')
        $video = Join-Path $videos "$clip-$look.mp4"
        $poster = Join-Path $videos "$clip-$look.webp"
        Invoke-Program 'ffmpeg' {
            ffmpeg -hide_banner -loglevel error -y -framerate 30 -i $pattern -c:v libx264 -preset slow -crf $Crf -tune animation -pix_fmt yuv420p -movflags +faststart -an $video
        }
        Invoke-Program 'ffmpeg' {
            ffmpeg -hide_banner -loglevel error -y -i $last.FullName -vf 'scale=1536:-2:flags=lanczos' -c:v libwebp -quality 82 $poster
        }
        '{0,-20} {1,6:N0} KB video {2,6:N0} KB poster {3,4} frames' -f "$clip-$look", ((Get-Item $video).Length / 1KB), ((Get-Item $poster).Length / 1KB), (Get-ChildItem $frames -Filter 'f*.png').Count
    }
}
$total = Get-Content (Join-Path $folders['cache'] 'total.txt') -ErrorAction SilentlyContinue
'{0:N1} MB fetched from the web in all runs (cap {1:N1} MB)' -f ([double]$total / 1MB), ($Cap / 1MB)
