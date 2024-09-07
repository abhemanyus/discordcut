-- Add migration script here
create table if not exists message (
    id integer primary key autoincrement,
    channelId integer not null,
    messageId integer not null,
    content text
);
create index if not exists channelIndex on message (channelId);
create index if not exists messageIndex on message (messageId);
