// No IRQs are enabled in this ROM (it polls VCOUNT), so the dispatcher never runs.
// It only exists because crt0.s's IntrMain calls it.
void irqDispatch(void) {}
