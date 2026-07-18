from routes.findings import bp as findings_bp
from routes.projects import bp as projects_bp
from routes.scans import bp as scans_bp
from routes.settings import bp as settings_bp
from routes.upload import bp as upload_bp


def register_blueprints(app):
    app.register_blueprint(projects_bp)
    app.register_blueprint(findings_bp)
    app.register_blueprint(scans_bp)
    app.register_blueprint(settings_bp)
    app.register_blueprint(upload_bp)
