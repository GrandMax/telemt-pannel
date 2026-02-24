"""CLI for panel: manage admin accounts (e.g. from install.sh)."""
import argparse
import sys

from app.database import SessionLocal, Base, engine
from app.models.admin import Admin
from app.utils.auth import hash_password


def cmd_create_admin(args: argparse.Namespace) -> int:
    Base.metadata.create_all(bind=engine)
    db = SessionLocal()
    try:
        if db.query(Admin).filter(Admin.username == args.username).first():
            print("error: username already exists", file=sys.stderr)
            return 1
        admin = Admin(
            username=args.username,
            hashed_password=hash_password(args.password),
            is_sudo=args.sudo,
        )
        db.add(admin)
        db.commit()
        print(f"Admin created: {args.username}" + (" (sudo)" if args.sudo else ""))
        return 0
    finally:
        db.close()


def cmd_reset_password(args: argparse.Namespace) -> int:
    Base.metadata.create_all(bind=engine)
    db = SessionLocal()
    try:
        admin = db.query(Admin).filter(Admin.username == args.username).first()
        if not admin:
            print(f"error: admin '{args.username}' not found", file=sys.stderr)
            return 1
        admin.hashed_password = hash_password(args.password)
        db.commit()
        print(f"Password updated for: {args.username}")
        return 0
    finally:
        db.close()


def main() -> int:
    parser = argparse.ArgumentParser(description="MTProxy Panel CLI")
    sub = parser.add_subparsers(dest="cmd", required=True)

    create = sub.add_parser("create-admin", help="Create an admin user")
    create.add_argument("--username", required=True, help="Admin username")
    create.add_argument("--password", required=True, help="Admin password")
    create.add_argument("--sudo", action="store_true", help="Sudo admin")
    create.set_defaults(func=cmd_create_admin)

    reset = sub.add_parser("reset-password", help="Reset admin password")
    reset.add_argument("--username", required=True, help="Admin username")
    reset.add_argument("--password", required=True, help="New password")
    reset.set_defaults(func=cmd_reset_password)

    args = parser.parse_args()
    return args.func(args)


if __name__ == "__main__":
    sys.exit(main())
