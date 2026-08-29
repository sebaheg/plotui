package plotui

/*
#include <plotui.h>
*/
import "C"

// Error carries the engine's status code and the exact message the Python
// binding would raise for the same mistake.
type Error struct {
	Code    int
	Message string
}

func (e *Error) Error() string { return e.Message }

// Status codes (mirroring plotui.h).
const (
	ErrInvalidArg    = int(C.PLOTUI_ERR_INVALID_ARG)
	ErrUnknownHandle = int(C.PLOTUI_ERR_UNKNOWN_HANDLE)
	ErrStructural    = int(C.PLOTUI_ERR_STRUCTURAL)
	ErrNull          = int(C.PLOTUI_ERR_NULL)
)

func statusErr(status C.int32_t) error {
	if status == C.PLOTUI_OK {
		return nil
	}
	return &Error{Code: int(status), Message: C.GoString(C.plotui_last_error())}
}
