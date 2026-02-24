"""Add user active_unique_ips

Revision ID: 003
Revises: 002
Create Date: 2025-02-24

"""
from typing import Sequence, Union
from alembic import op
import sqlalchemy as sa

revision: str = "003"
down_revision: Union[str, None] = "002"
branch_labels: Union[str, Sequence[str], None] = None
depends_on: Union[str, Sequence[str], None] = None


def upgrade() -> None:
    op.add_column("users", sa.Column("active_unique_ips", sa.Integer(), nullable=True))


def downgrade() -> None:
    op.drop_column("users", "active_unique_ips")
