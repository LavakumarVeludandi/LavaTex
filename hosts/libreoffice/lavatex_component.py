"""UNO component behind the LavaTeX menu entry and toolbar button."""

import traceback

import unohelper
from com.sun.star.task import XJobExecutor

IMPLEMENTATION_NAME = "org.lavatex.LibreOffice.Job"
SERVICE_NAME = "org.lavatex.LibreOffice"


class LavaTeXJob(unohelper.Base, XJobExecutor):
    def __init__(self, ctx):
        self.ctx = ctx

    def trigger(self, args):
        try:
            from lavatex_lo import app

            app.run(self.ctx)
        except Exception:
            # Never let an exception escape into LibreOffice; show it instead.
            try:
                from lavatex_lo.app import _message

                desktop = self.ctx.ServiceManager.createInstanceWithContext("com.sun.star.frame.Desktop", self.ctx)
                _message(self.ctx, desktop.getCurrentComponent(), "LavaTeX ran into a problem:\n\n" + traceback.format_exc())
            except Exception:
                traceback.print_exc()


g_ImplementationHelper = unohelper.ImplementationHelper()
g_ImplementationHelper.addImplementation(LavaTeXJob, IMPLEMENTATION_NAME, (SERVICE_NAME,))
