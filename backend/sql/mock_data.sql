-- Simple mock data for testing
INSERT INTO
    teams (id, name, created_at, updated_at, is_manual_upgrade)
SELECT
    1,
    'Dunder Mifflin',
    NOW(),
    NOW(),
    true
WHERE
    NOT EXISTS (
        SELECT
            1
        FROM
            teams
        WHERE
            id = 1
    );

INSERT INTO
    "public"."users" (
        "id",
        "first_name",
        "last_name",
        "email",
        "is_admin",
        "team_id",
        "hashed_password",
        "avatar_url",
        "created_at",
        "updated_at",
        "social_metadata",
        "unsubscribe_id"
    )
VALUES
    (
        '0195013f-20b5-719d-ac6b-f4beed3ba2ea',
        'Michael',
        'Scott',
        'michael@dundermifflin.com',
        'true',
        '1',
        '$2a$10$d6Kfs1rGlv4JGY12U.XfUOvKVaYVj2Au.SB3RT9M57m.j0Z/XvONG', -- hashed version of 'hoppless'
        'https://tvline.com/wp-content/uploads/2011/04/greatscott_april27_514110427100239.jpg?w=514&h=360&crop=1',
        NOW(),
        NOW(),
        null,
        '9969ac04-24a0-4fd4-9f22-2302406b1706'
    ),
    (
        '0195013f-bf8a-706f-a4f0-11d87ef40fce',
        'Dwight',
        'Schrute',
        'dwight@dundermifflin.com',
        'false',
        '1',
        '$2a$10$d6Kfs1rGlv4JGY12U.XfUOvKVaYVj2Au.SB3RT9M57m.j0Z/XvONG', -- hashed version of 'hoppless'
        'https://www.myany.city/sites/default/files/styles/scaled_cropped_medium__260x260/public/field/image/node-related-images/sample-dwight-k-schrute.jpg?itok=8TfRscbA',
        NOW(),
        NOW(),
        null,
        '9a5fc5f7-63a5-46ec-b50a-e4808d79e69f'
    ) ON CONFLICT (id) DO NOTHING;

