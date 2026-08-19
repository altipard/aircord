// Command genicon rasterizes assets/icon.svg to assets/icon.png using the same
// SVG renderer Fyne uses (oksvg/rasterx), so the embedded PNG looks exactly as
// Fyne would draw the vector. Run from the repo root after editing the SVG:
//
//	go run ./tools/genicon
package main

import (
	"image"
	"image/png"
	"log"
	"os"

	"github.com/srwiley/oksvg"
	"github.com/srwiley/rasterx"
)

const (
	src  = "assets/icon.svg"
	dst  = "assets/icon.png"
	size = 512
)

func main() {
	icon, err := oksvg.ReadIcon(src, oksvg.WarnErrorMode)
	if err != nil {
		log.Fatalf("read %s: %v", src, err)
	}
	icon.SetTarget(0, 0, float64(size), float64(size))

	rgba := image.NewRGBA(image.Rect(0, 0, size, size))
	scanner := rasterx.NewScannerGV(size, size, rgba, rgba.Bounds())
	raster := rasterx.NewDasher(size, size, scanner)
	icon.Draw(raster, 1.0)

	f, err := os.Create(dst)
	if err != nil {
		log.Fatalf("create %s: %v", dst, err)
	}
	if err := png.Encode(f, rgba); err != nil {
		_ = f.Close()
		log.Fatalf("encode %s: %v", dst, err)
	}
	if err := f.Close(); err != nil {
		log.Fatalf("close %s: %v", dst, err)
	}
	log.Printf("wrote %s (%dx%d)", dst, size, size)
}
