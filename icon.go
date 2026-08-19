package main

import (
	_ "embed"

	"fyne.io/fyne/v2"
)

//go:embed assets/icon.png
var iconPNG []byte

// appIcon is the embedded window / dock / taskbar icon. The editable source is
// assets/icon.svg; regenerate the PNG with `go run ./tools/genicon`.
var appIcon = fyne.NewStaticResource("icon.png", iconPNG)
