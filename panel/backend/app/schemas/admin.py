from pydantic import BaseModel, Field


class AdminCreate(BaseModel):
    username: str = Field(..., min_length=1, max_length=64)
    password: str = Field(..., min_length=1)
    is_sudo: bool = False


class AdminResponse(BaseModel):
    id: int
    username: str
    is_sudo: bool
    created_at: str

    class Config:
        from_attributes = True


class AdminInDB(AdminResponse):
    hashed_password: str


class Token(BaseModel):
    access_token: str
    token_type: str = "bearer"


class TokenForm(BaseModel):
    username: str
    password: str
