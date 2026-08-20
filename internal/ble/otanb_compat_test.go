package ble_test

import (
	"github.com/altipard/aircord/internal/ble"
	"github.com/altipard/aircord/internal/otanb"
)

// *ble.OTASession must satisfy otanb.Transport so the main/UI layer can drive
// otanb.Upgrade over a live BLE connection. This is a compile-time guarantee
// only; it adds no production dependency between the packages.
var _ otanb.Transport = (*ble.OTASession)(nil)
